/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::collections::VecDeque;

use ion::class::Reflector;
use ion::conversions::ToValue as _;
use ion::function::Opt;
use ion::typedarray::{ArrayBufferView, type_to_constructor, type_to_element_size};
use ion::{
	ClassDefinition as _, Context, Error, ErrorKind, FromValue, Local, Object, Promise, Result, ResultExc, Traceable,
	Value, js_class,
};
use mozjs::conversions::ConversionBehavior;
use mozjs::jsapi::{Heap, JSObject};

use crate::globals::streams::readable::controller::{ControllerInternals as _, ControllerKind, PullIntoDescriptor};
use crate::globals::streams::readable::{ReadableStream, State};

pub type ChunkErrorClosure = dyn Fn(&Context, &Promise, &Value);
pub type CloseClosure = dyn Fn(&Context, &Promise, Option<&Value>) -> ResultExc<()>;

#[derive(Traceable)]
pub struct Request {
	pub(crate) promise: Box<Heap<*mut JSObject>>,
	#[trace(no_trace)]
	pub(crate) chunk: Box<ChunkErrorClosure>,
	#[trace(no_trace)]
	pub(crate) close: Box<CloseClosure>,
	#[trace(no_trace)]
	pub(crate) error: Box<ChunkErrorClosure>,
}

impl Request {
	pub(crate) fn standard(promise: *mut JSObject) -> Request {
		struct ReadResult<'cx> {
			pub value: Option<Value<'cx>>,
			pub done: bool,
		}

		fn into_value<'cx>(result: ReadResult, cx: &'cx Context) -> Value<'cx> {
			let object = Object::new(cx);
			object.set(cx, "value", &result.value.unwrap_or_else(Value::undefined_handle));
			object.set_as(cx, "done", &result.done);
			object.as_value(cx)
		}

		Request {
			promise: Heap::boxed(promise),
			chunk: Box::new(|cx, promise, chunk| {
				let result = ReadResult {
					value: Some(Value::from(Local::from_handle(chunk.handle()))),
					done: false,
				};
				promise.resolve(cx, &into_value(result, cx));
			}),
			close: Box::new(|cx, promise, chunk| {
				let result = ReadResult {
					value: chunk.map(|v| Value::from(Local::from_handle(v.handle()))),
					done: true,
				};
				promise.resolve(cx, &into_value(result, cx));
				Ok(())
			}),
			error: Box::new(|cx, promise, error| {
				promise.resolve(cx, error);
			}),
		}
	}

	pub(crate) fn promise(&self) -> Promise {
		Promise::from(unsafe { Local::from_heap(&self.promise) }).unwrap()
	}
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Traceable)]
pub enum ReaderKind {
	None,
	Default,
	Byob,
}

pub enum Reader<'r> {
	Default(&'r mut DefaultReader),
	Byob(&'r mut ByobReader),
}

impl<'r> Reader<'r> {
	pub(crate) fn common(&self) -> &CommonReader {
		match self {
			Reader::Default(reader) => &reader.common,
			Reader::Byob(reader) => &reader.common,
		}
	}

	pub(crate) fn into_default(self) -> Option<&'r mut DefaultReader> {
		match self {
			Reader::Default(reader) => Some(reader),
			Reader::Byob(_) => None,
		}
	}

	pub(crate) fn into_byob(self) -> Option<&'r mut ByobReader> {
		match self {
			Reader::Byob(reader) => Some(reader),
			Reader::Default(_) => None,
		}
	}

	pub fn requests_closed(self) -> (&'r mut VecDeque<Request>, Promise<'r>) {
		let common = match self {
			Reader::Default(reader) => &mut reader.common,
			Reader::Byob(reader) => &mut reader.common,
		};
		unsafe {
			(
				&mut common.requests,
				Promise::from(Local::from_heap(&common.closed)).unwrap(),
			)
		}
	}

	pub fn is_empty(&self) -> bool {
		self.common().requests.is_empty()
	}
}

#[js_class]
#[ion(name = "ReadableStreamCommonReader")]
pub struct CommonReader {
	reflector: Reflector,

	stream: Option<Box<Heap<*mut JSObject>>>,
	pub(crate) requests: VecDeque<Request>,
	pub(crate) closed: Box<Heap<*mut JSObject>>,
}

#[js_class]
impl CommonReader {
	pub(crate) fn new(cx: &Context, stream: &ReadableStream, stream_object: &Object) -> CommonReader {
		let closed = Promise::new(cx);
		match stream.state {
			State::Readable => {}
			State::Closed => {
				closed.resolve(cx, &Value::undefined_handle());
			}
			State::Errored => {
				closed.reject(cx, &stream.stored_error());
			}
		}

		CommonReader {
			reflector: Reflector::default(),
			stream: Some(Heap::boxed(stream_object.handle().get())),
			requests: VecDeque::new(),
			closed: Heap::boxed(closed.get()),
		}
	}

	pub(crate) fn stream(&self, cx: &Context) -> Result<Option<&mut ReadableStream>> {
		self.stream
			.as_ref()
			.map::<Result<_>, _>(|stream| {
				let stream = Object::from(unsafe { Local::from_heap(stream) });
				ReadableStream::get_mut_private(cx, &stream)
			})
			.transpose()
	}

	pub(crate) fn closed(&self) -> Promise {
		Promise::from(unsafe { Local::from_heap(&self.closed) }).unwrap()
	}

	pub(crate) fn cancel<'cx>(&self, cx: &'cx Context, reason: Opt<Value>) -> ResultExc<Promise<'cx>> {
		if let Some(stream) = self.stream(cx)? {
			stream.cancel(cx, reason)
		} else {
			let promise = Promise::new(cx);
			promise.reject_with_error(cx, &Error::new("Reader has already been released.", ErrorKind::Type));
			Ok(promise)
		}
	}

	pub(crate) fn release_lock(&mut self, cx: &Context) -> Result<()> {
		if let Some(stream) = self.stream(cx)? {
			let mut closed = self.closed();
			if matches!(stream.state, State::Closed | State::Errored) {
				self.closed.set(Promise::new(cx).get());
				closed = self.closed();
			}
			closed.reject_with_error(cx, &Error::new("Released Reader", ErrorKind::Type));

			stream.reader_kind = ReaderKind::None;
			stream.reader = None;

			stream.native_controller(cx)?.release();

			while let Some(request) = self.requests.pop_front() {
				let promise = request.promise();
				(request.error)(
					cx,
					&promise,
					&Error::new("Reader has been released.", ErrorKind::Type).as_value(cx),
				);
			}
		} else {
			return Err(Error::new("Reader has already been released.", ErrorKind::Type));
		}
		self.stream = None;
		Ok(())
	}
}

#[js_class]
#[ion(name = "ReadableStreamDefaultReader")]
pub struct DefaultReader {
	pub(crate) common: CommonReader,
}

#[js_class]
impl DefaultReader {
	#[ion(constructor)]
	pub fn constructor(cx: &Context, #[ion(this)] this: &Object, stream_object: Object) -> Result<DefaultReader> {
		let reader = DefaultReader::new(cx, &stream_object)?;
		let stream = ReadableStream::get_mut_private(cx, &stream_object)?;
		stream.reader_kind = ReaderKind::Default;
		stream.reader = Some(Heap::boxed(this.handle().get()));

		Ok(reader)
	}

	pub(crate) fn new(cx: &Context, stream_object: &Object) -> Result<DefaultReader> {
		let stream = ReadableStream::get_private(cx, stream_object)?;
		if stream.get_locked() {
			return Err(Error::new(
				"Cannot create DefaultReader from locked stream.",
				ErrorKind::Type,
			));
		}

		Ok(DefaultReader {
			common: CommonReader::new(cx, stream, stream_object),
		})
	}

	pub(crate) fn read_internal<'cx>(&mut self, cx: &'cx Context, request: Request) -> ResultExc<Promise<'cx>> {
		let promise = Promise::from(cx.root(request.promise.get())).unwrap();
		if let Some(stream) = self.common.stream(cx)? {
			stream.disturbed = true;

			match stream.state {
				State::Readable => stream.native_controller(cx)?.pull(cx, &promise, request)?,
				State::Closed => (request.close)(cx, &promise, None)?,
				State::Errored => (request.error)(cx, &promise, &stream.stored_error()),
			}
		} else {
			promise.reject_with_error(cx, &Error::new("Reader has already been released.", ErrorKind::Type));
		}
		Ok(promise)
	}

	pub fn cancel<'cx>(&self, cx: &'cx Context, reason: Opt<Value>) -> ResultExc<Promise<'cx>> {
		self.common.cancel(cx, reason)
	}

	pub fn read<'cx>(&mut self, cx: &'cx Context) -> ResultExc<Promise<'cx>> {
		let promise = Promise::new(cx);
		self.read_internal(cx, Request::standard(promise.get()))
	}

	#[ion(name = "releaseLock")]
	pub fn release_lock(&mut self, cx: &Context) -> Result<()> {
		self.common.release_lock(cx)
	}

	#[ion(get)]
	pub fn get_closed(&self) -> *mut JSObject {
		self.common.closed.get()
	}
}

#[derive(FromValue)]
pub struct ByobReadOptions {
	#[ion(convert = ConversionBehavior::EnforceRange, default = 1)]
	min: u64,
}

impl Default for ByobReadOptions {
	fn default() -> ByobReadOptions {
		ByobReadOptions { min: 1 }
	}
}

#[js_class]
#[ion(name = "ReadableStreamBYOBReader")]
pub struct ByobReader {
	pub(crate) common: CommonReader,
}

#[js_class]
impl ByobReader {
	#[ion(constructor)]
	pub fn constructor(cx: &Context, #[ion(this)] this: &Object, stream_object: Object) -> Result<ByobReader> {
		let reader = ByobReader::new(cx, &stream_object)?;
		let stream = ReadableStream::get_mut_private(cx, &stream_object)?;
		stream.reader_kind = ReaderKind::Byob;
		stream.reader = Some(Heap::boxed(this.handle().get()));

		Ok(reader)
	}

	pub(crate) fn new(cx: &Context, stream_object: &Object) -> Result<ByobReader> {
		let stream = ReadableStream::get_private(cx, stream_object)?;
		if stream.get_locked() {
			return Err(Error::new(
				"Cannot create BYOBReader from locked stream.",
				ErrorKind::Type,
			));
		}

		if stream.controller_kind == ControllerKind::Default {
			return Err(Error::new(
				"Cannot create BYOBReader from DefaultController",
				ErrorKind::Type,
			));
		}

		Ok(ByobReader {
			common: CommonReader::new(cx, stream, stream_object),
		})
	}

	pub(crate) fn read_internal<'cx>(
		&mut self, cx: &'cx Context, view: ArrayBufferView, min: usize, request: Request,
	) -> ResultExc<Promise<'cx>> {
		let stream = self.common.stream(cx)?.unwrap();
		let promise = Promise::from(cx.root(request.promise.get())).unwrap();

		stream.disturbed = true;
		if stream.state == State::Errored {
			(request.error)(cx, &promise, &stream.stored_error());
			return Ok(promise);
		}

		let (constructor, element_size) = {
			let ty = view.view_type();
			(type_to_constructor(ty), type_to_element_size(ty))
		};

		let offset = view.offset();
		let length = view.len();
		let buffer = view.buffer(cx);
		match buffer.transfer(cx) {
			Ok(buffer) => {
				let mut descriptor = PullIntoDescriptor {
					buffer: Heap::boxed(buffer.get()),
					offset,
					length: length * element_size,
					filled: 0,
					min: min * element_size,
					element: element_size,
					constructor,
					kind: ReaderKind::Byob,
				};

				let controller = stream.native_controller(cx)?.into_byte_stream().unwrap();
				if !controller.pending_descriptors.is_empty() {
					controller.pending_descriptors.push_back(descriptor);

					if stream.state == State::Readable {
						self.common.requests.push_back(request);
					}
					return Ok(promise);
				} else if stream.state == State::Closed {
					let empty = descriptor.construct(cx)?.as_value(cx);
					(request.close)(cx, &promise, Some(&empty))?;
					return Ok(promise);
				} else if controller.common.queue_size > 0 {
					if controller.fill_pull_into_descriptor(cx, &mut descriptor)? {
						let buffer = buffer.transfer(cx)?;
						descriptor.buffer.set(buffer.get());
						let view = descriptor.construct(cx)?.as_value(cx);

						if controller.common.queue_size == 0 && controller.common.close_requested {
							controller.close(cx)?;
						} else {
							controller.pull_if_needed(cx)?;
						}

						(request.chunk)(cx, &promise, &view);
						return Ok(promise);
					} else if controller.common.close_requested {
						let error = Error::new("Stream closed by request.", ErrorKind::Type).as_value(cx);
						let _ = controller.error_internal(cx, &error);
						(request.error)(cx, &promise, &error);
						return Ok(promise);
					}
				}

				controller.pending_descriptors.push_back(descriptor);
				if stream.state == State::Readable {
					self.common.requests.push_back(request);
				}

				let stream = self.common.stream(cx)?.unwrap();
				let controller = stream.native_controller(cx)?.into_byte_stream().unwrap();
				controller.pull_if_needed(cx)?;
			}
			Err(error) => {
				(request.error)(cx, &promise, &error.as_value(cx));
			}
		}
		Ok(promise)
	}

	pub fn cancel<'cx>(&self, cx: &'cx Context, reason: Opt<Value>) -> ResultExc<Promise<'cx>> {
		self.common.cancel(cx, reason)
	}

	pub fn read<'cx>(
		&mut self, cx: &'cx Context, view: ArrayBufferView, Opt(options): Opt<ByobReadOptions>,
	) -> ResultExc<Promise<'cx>> {
		let promise = Promise::new(cx);
		let request = Request::standard(promise.get());
		if self.common.stream.is_some() {
			if view.is_empty() {
				promise.reject(cx, &Error::new("View must not be empty.", ErrorKind::Type).as_value(cx));
				return Ok(promise);
			}

			let buffer = view.buffer(cx);

			if buffer.is_empty() {
				promise.reject(
					cx,
					&Error::new("Buffer must contain bytes.", ErrorKind::Type).as_value(cx),
				);
				return Ok(promise);
			}

			if buffer.is_detached() {
				promise.reject(
					cx,
					&Error::new("ArrayBuffer must not be detached.", ErrorKind::Type).as_value(cx),
				);
				return Ok(promise);
			}

			let options = options.unwrap_or_default();
			if options.min == 0 {
				promise.reject(
					cx,
					&Error::new("min must be greater than 0.", ErrorKind::Type).as_value(cx),
				);
				return Ok(promise);
			}

			if options.min > view.len() as u64 {
				promise.reject(
					cx,
					&Error::new("min is greater than View Length", ErrorKind::Range).as_value(cx),
				);
				return Ok(promise);
			}

			self.read_internal(cx, view, options.min as usize, request)
		} else {
			(request.error)(
				cx,
				&promise,
				&Error::new("Reader has already been released.", ErrorKind::Type).as_value(cx),
			);
			Ok(Promise::new(cx))
		}
	}

	#[ion(name = "releaseLock")]
	pub fn release_lock(&mut self, cx: &Context) -> Result<()> {
		self.common.release_lock(cx)
	}

	#[ion(get)]
	pub fn get_closed(&self) -> *mut JSObject {
		self.common.closed.get()
	}
}
