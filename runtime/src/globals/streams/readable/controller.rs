/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::collections::VecDeque;
use std::ptr;

use ion::class::{NativeObject as _, Reflector};
use ion::conversions::{FromValue as _, ToValue as _};
use ion::typedarray::{ArrayBuffer, ArrayBufferView, Uint8Array, type_to_constructor};
use ion::{
	ClassDefinition, Context, Error, ErrorKind, Exception, Function, Local, Object, Promise, Result, ResultExc,
	Traceable, TracedHeap, Value, js_class,
};
use mozjs::conversions::ConversionBehavior;
use mozjs::jsapi::{Handle, Heap, JSContext, JSObject, Type};
use mozjs::jsval::{DoubleValue, Int32Value, JSVal, NullValue, UndefinedValue};

use crate::globals::streams::readable::reader::{Reader, ReaderKind, Request};
use crate::globals::streams::readable::{ByobReader, ReadableStream, State, StreamSource, UnderlyingSource};
use crate::globals::streams::strategy::{QueuingStrategy, SizeCallback};

#[derive(Traceable)]
pub(crate) struct PullIntoDescriptor {
	pub(crate) buffer: Box<Heap<*mut JSObject>>,
	pub(crate) offset: usize,
	pub(crate) length: usize,
	pub(crate) filled: usize,
	pub(crate) min: usize,
	pub(crate) element: usize,
	pub(crate) constructor: unsafe extern "C" fn(*mut JSContext, Handle<*mut JSObject>, usize, i64) -> *mut JSObject,
	pub(crate) kind: ReaderKind,
}

impl PullIntoDescriptor {
	pub(crate) fn buffer(&self) -> ArrayBuffer<'_> {
		ArrayBuffer::from(unsafe { Local::from_heap(&self.buffer) }).unwrap()
	}

	pub(crate) fn construct<'cx>(&self, cx: &'cx Context) -> Result<ArrayBufferView<'cx>> {
		let view = unsafe {
			(self.constructor)(
				cx.as_ptr(),
				self.buffer.handle(),
				self.offset,
				(self.filled / self.element) as i64,
			)
		};

		if !view.is_null() {
			Ok(ArrayBufferView::from(cx.root(view)).unwrap())
		} else if let Some(Exception::Error(exception)) = Exception::new(cx)? {
			Err(exception)
		} else {
			Err(Error::new("Failed to Initialise Array Buffer", None))
		}
	}

	pub(crate) fn commit(&mut self, cx: &Context, reader: &mut ByobReader, state: State) -> ResultExc<()> {
		let mut done = false;

		let buffer = self.buffer();
		if state == State::Closed {
			done = true;
		}
		let buffer = buffer.transfer(cx)?;

		self.buffer.set(buffer.get());
		let view = self.construct(cx)?.as_value(cx);

		let request = reader.common.requests.pop_front().unwrap();
		let promise = request.promise();

		if !done {
			(request.chunk)(cx, &promise, &view);
		} else {
			(request.close)(cx, &promise, Some(&view))?;
		}
		Ok(())
	}
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Traceable)]
pub enum ControllerKind {
	Default,
	ByteStream,
}

pub enum Controller<'c> {
	Default(&'c mut DefaultController),
	ByteStream(&'c mut ByteStreamController),
}

impl<'c> Controller<'c> {
	pub(crate) fn common_mut(&mut self) -> &mut CommonController {
		match self {
			Controller::Default(controller) => &mut controller.common,
			Controller::ByteStream(controller) => &mut controller.common,
		}
	}

	pub(crate) fn into_default(self) -> Option<&'c mut DefaultController> {
		match self {
			Controller::Default(controller) => Some(controller),
			Controller::ByteStream(_) => None,
		}
	}

	pub(crate) fn into_byte_stream(self) -> Option<&'c mut ByteStreamController> {
		match self {
			Controller::ByteStream(controller) => Some(controller),
			Controller::Default(_) => None,
		}
	}

	pub fn cancel<'cx: 'v, 'v>(&mut self, cx: &'cx Context, reason: Option<Value<'v>>) -> ResultExc<Promise<'cx>> {
		match self {
			Controller::Default(controller) => controller.reset_queue(cx),
			Controller::ByteStream(controller) => controller.reset_queue(cx),
		}
		let common = self.common_mut();

		let mut promise = Promise::new(cx);
		common.source.cancel(cx, &mut promise, reason)?;
		common.source.clear_algorithms();
		Ok(promise)
	}

	pub fn pull(&mut self, cx: &Context, promise: &Promise, request: Request) -> ResultExc<()> {
		match self {
			Controller::Default(controller) => {
				if let Some((chunk, size)) = controller.queue.pop_front() {
					controller.common.queue_size -= size as usize;

					if controller.common.close_requested && controller.queue.is_empty() {
						controller.common.source.clear_algorithms();
						controller.size = SizeCallback::None;

						let stream = controller.common.stream(cx)?;
						stream.close(cx)?;
					} else {
						controller.pull_if_needed(cx)?;
					}

					let chunk = Value::from(unsafe { Local::from_heap(&chunk) });
					(request.chunk)(cx, promise, &chunk);
				} else {
					let stream = controller.common.stream(cx)?;
					match stream.native_reader(cx)? {
						Some(Reader::Default(reader)) => {
							if stream.state != State::Readable {
								return Err(Error::new("Cannot Add Read Request to Read Queue", None).into());
							}
							reader.common.requests.push_back(request);
						}
						_ => return Ok(()),
					}
					controller.pull_if_needed(cx)?;
				}
			}
			Controller::ByteStream(controller) => {
				{
					let stream = controller.common.stream(cx)?;
					if stream.reader_kind != ReaderKind::Default {
						return Err(Error::new("Reader should have default reader.", ErrorKind::Type).into());
					}
				}

				if controller.common.queue_size > 0 {
					let (buffer, offset, length) = controller.queue.pop_front().unwrap();
					controller.common.queue_size -= length;

					if controller.common.queue_size == 0 && controller.common.close_requested {
						controller.close(cx)?;
					} else {
						controller.pull_if_needed(cx)?;
					}

					let buffer = ArrayBuffer::from(unsafe { Local::from_heap(&buffer) }).unwrap();
					let array = Uint8Array::with_array_buffer(cx, &buffer, offset, length).unwrap().as_value(cx);

					(request.chunk)(cx, promise, &array);
				} else {
					if controller.auto_allocate_chunk_size != 0 {
						let Some(buffer) = ArrayBuffer::new(cx, controller.auto_allocate_chunk_size) else {
							controller.error_internal(cx, &Exception::new(cx).unwrap().as_value(cx))?;
							return Ok(());
						};

						controller.pending_descriptors.push_back(PullIntoDescriptor {
							buffer: Heap::boxed(buffer.get()),
							offset: 0,
							length: controller.auto_allocate_chunk_size,
							filled: 0,
							min: 1,
							element: 1,
							constructor: type_to_constructor(Type::Uint8),
							kind: ReaderKind::Default,
						});
					}

					let stream = controller.common.stream(cx)?;
					if let Some(Reader::Default(reader)) = stream.native_reader(cx)? {
						reader.common.requests.push_back(request);
					}
					controller.pull_if_needed(cx)?;
				}
			}
		}
		Ok(())
	}

	pub fn release(&mut self) {
		match self {
			Controller::Default(_) => {}
			Controller::ByteStream(controller) => {
				if let Some(mut descriptor) = controller.pending_descriptors.pop_front() {
					controller.pending_descriptors.clear();
					descriptor.kind = ReaderKind::None;
					controller.pending_descriptors.push_back(descriptor);
				}
			}
		}
	}
}

#[js_class]
#[ion(name = "ReadableStreamCommonController")]
pub struct CommonController {
	reflector: Reflector,

	pub(crate) stream: Box<Heap<*mut JSObject>>,
	pub(crate) source: StreamSource,

	pub(crate) started: bool,
	pub(crate) pulling: bool,
	pub(crate) pull_again: bool,
	pub(crate) close_requested: bool,

	high_water_mark: f64,
	pub(crate) queue_size: usize,
}

impl CommonController {
	pub fn new(stream: &Object, source: StreamSource, high_water_mark: f64) -> CommonController {
		CommonController {
			reflector: Reflector::default(),

			stream: Heap::boxed(stream.handle().get()),
			source,

			started: false,
			pulling: false,
			pull_again: false,
			close_requested: false,

			high_water_mark,
			queue_size: 0,
		}
	}
}

#[js_class]
impl CommonController {
	pub(crate) fn new_from_script(
		stream: &Object, source_object: Option<&Object>, source: &UnderlyingSource, high_water_mark: f64,
	) -> CommonController {
		CommonController::new(stream, source.to_native(source_object), high_water_mark)
	}

	#[expect(clippy::mut_from_ref)]
	pub(crate) fn stream<'cx>(&self, cx: &'cx Context) -> Result<&'cx mut ReadableStream> {
		let stream = Object::from(unsafe { Local::from_heap(&*ptr::from_ref(&self.stream)) });
		ReadableStream::get_mut_private(cx, &stream)
	}

	pub(crate) fn start<C: ControllerInternals>(&mut self, cx: &Context, start: Option<&Function>) -> ResultExc<()> {
		let controller = self.reflector().get();

		let underlying_source = self.source.source_object();
		let value = controller.as_value(cx);
		let result = start
			.map_or_else(
				|| Ok(UndefinedValue()),
				|start| start.call(cx, &underlying_source, &[value]).map(|v| v.get()),
			)
			.map_err(|report| report.unwrap().exception)?;

		let promise = Promise::resolved(cx, &Value::from(cx.root(result)));

		let controller1 = TracedHeap::new(controller);
		let controller2 = TracedHeap::new(controller);
		promise.add_reactions(
			cx,
			move |cx, _| {
				let controller = C::from_traced_heap(cx, &controller1)?;
				controller.common().started = true;
				controller.pull_if_needed(cx)?;
				Ok(Value::undefined_handle())
			},
			move |cx, error| {
				let controller = C::from_traced_heap(cx, &controller2)?;
				controller.error_internal(cx, error)?;
				Ok(Value::undefined_handle())
			},
		);

		Ok(())
	}

	pub(crate) fn can_close_or_enqueue(&self, stream: &ReadableStream) -> bool {
		stream.state == State::Readable && !self.close_requested
	}

	pub(crate) fn should_call_pull(&self, cx: &Context, stream: &ReadableStream) -> Result<bool> {
		if !self.can_close_or_enqueue(stream) || !self.started {
			return Ok(false);
		}

		if let Some(reader) = stream.native_reader(cx)?
			&& !reader.is_empty()
		{
			return Ok(true);
		}
		Ok(stream.state == State::Readable && self.high_water_mark > self.queue_size as f64)
	}

	pub(crate) fn pull_if_needed<C: ControllerInternals>(&mut self, cx: &Context) -> ResultExc<()> {
		let stream = self.stream(cx)?;
		if !self.should_call_pull(cx, stream)? {
			return Ok(());
		}

		if self.pulling {
			self.pull_again = true;
			return Ok(());
		}

		self.pulling = true;

		let promise = self.source.pull(cx, self.reflector.get())?;
		if let Some(promise) = promise {
			let controller1 = TracedHeap::new(stream.controller.get());
			let controller2 = TracedHeap::new(stream.controller.get());

			promise.add_reactions(
				cx,
				move |cx, _| {
					let controller = C::from_traced_heap(cx, &controller1)?;
					controller.common().pulling = false;
					if controller.common().pull_again {
						controller.common().pull_again = false;
						controller.pull_if_needed(cx)?;
					}
					Ok(Value::undefined_handle())
				},
				move |cx, error| {
					let controller = C::from_traced_heap(cx, &controller2)?;
					controller.error_internal(cx, error)?;
					Ok(Value::undefined_handle())
				},
			);
		}
		Ok(())
	}

	pub(crate) fn desired_size(&self, cx: &Context) -> Result<JSVal> {
		let size = match self.stream(cx)?.state {
			State::Readable => DoubleValue(self.high_water_mark - self.queue_size as f64),
			State::Closed => Int32Value(0),
			State::Errored => NullValue(),
		};
		Ok(size)
	}
}

pub(crate) trait ControllerInternals: ClassDefinition {
	fn from_traced_heap<'h>(cx: &Context, heap: &'h TracedHeap<*mut JSObject>) -> Result<&'h mut Self> {
		let controller = Object::from(heap.to_local());
		Self::get_mut_private(cx, &controller)
	}

	fn common(&mut self) -> &mut CommonController;

	fn start(&mut self, cx: &Context, start: Option<&Function>) -> ResultExc<()> {
		self.common().start::<Self>(cx, start)
	}

	fn pull_if_needed(&mut self, cx: &Context) -> ResultExc<()> {
		self.common().pull_if_needed::<Self>(cx)
	}

	fn reset_queue(&mut self, cx: &Context);

	fn clear_algorithms(&mut self) {
		self.common().source.clear_algorithms();
	}

	fn error_internal(&mut self, cx: &Context, error: &Value) -> Result<()> {
		if self.common().stream(cx)?.state == State::Readable {
			self.reset_queue(cx);
			self.clear_algorithms();
			self.common().stream(cx)?.error(cx, error)
		} else {
			Ok(())
		}
	}
}

#[js_class]
#[ion(name = "ReadableStreamDefaultController")]
pub struct DefaultController {
	pub(crate) common: CommonController,
	pub(crate) size: SizeCallback,
	pub(crate) queue: VecDeque<(Box<Heap<JSVal>>, u64)>,
}

#[js_class]
impl DefaultController {
	pub(crate) fn initialise(
		stream: &Object, source_object: Option<&Object>, source: &UnderlyingSource, strategy: &QueuingStrategy,
		high_water_mark: f64,
	) -> DefaultController {
		DefaultController {
			common: CommonController::new_from_script(stream, source_object, source, high_water_mark),
			size: strategy.size(),
			queue: VecDeque::new(),
		}
	}

	#[ion(get)]
	pub fn get_desired_size(&self, cx: &Context) -> Result<JSVal> {
		self.common.desired_size(cx)
	}

	pub fn close(&mut self, cx: &Context) -> ResultExc<()> {
		let stream = self.common.stream(cx)?;
		if self.common.can_close_or_enqueue(stream) {
			if self.queue.is_empty() {
				self.common.close_requested = true;
			}
			self.common.source.clear_algorithms();
			self.size = SizeCallback::None;

			stream.close(cx)
		} else {
			Err(Error::new("Cannot Close Stream", ErrorKind::Type).into())
		}
	}

	pub fn enqueue(&mut self, cx: &Context, chunk: Value) -> ResultExc<()> {
		self.enqueue_internal(cx, &chunk)
	}

	pub(crate) fn enqueue_internal(&mut self, cx: &Context, chunk: &Value) -> ResultExc<()> {
		let stream = self.common.stream(cx)?;
		if self.common.can_close_or_enqueue(stream) {
			if let Some(Reader::Default(reader)) = stream.native_reader(cx)?
				&& let Some(request) = reader.common.requests.pop_front()
			{
				let promise = request.promise();
				(request.chunk)(cx, &promise, chunk);
				return Ok(());
			}

			match self.size.compute(cx, chunk) {
				Ok(size) => {
					let size = u64::from_value(cx, &size, false, ConversionBehavior::EnforceRange);
					match size {
						Ok(size) => {
							self.queue.push_back((Heap::boxed(chunk.get()), size));
							self.common.queue_size += size as usize;
							self.pull_if_needed(cx)?;
						}
						Err(error) => {
							self.error_internal(cx, &error.as_value(cx))?;
						}
					}
				}
				Err(exception) => {
					self.error_internal(cx, &exception.as_value(cx))?;
				}
			}
			Ok(())
		} else {
			Err(Error::new("Cannot Enqueue to Stream", ErrorKind::Type).into())
		}
	}

	pub fn error(&mut self, cx: &Context, error: Option<Value>) -> Result<()> {
		self.error_internal(cx, &error.unwrap_or_else(Value::undefined_handle))
	}
}

impl ControllerInternals for DefaultController {
	fn common(&mut self) -> &mut CommonController {
		&mut self.common
	}

	fn reset_queue(&mut self, _: &Context) {
		self.queue.clear();
		self.common.queue_size = 0;
	}

	fn clear_algorithms(&mut self) {
		self.common().source.clear_algorithms();
		self.size = SizeCallback::None;
	}
}

#[js_class]
#[ion(name = "ReadableByteStreamController")]
pub struct ByteStreamController {
	pub(crate) common: CommonController,
	pub(crate) auto_allocate_chunk_size: usize,
	pub(crate) byob_request: Option<Box<Heap<*mut JSObject>>>,
	pub(crate) pending_descriptors: VecDeque<PullIntoDescriptor>,
	pub(crate) queue: VecDeque<(Box<Heap<*mut JSObject>>, usize, usize)>,
}

#[js_class]
impl ByteStreamController {
	pub(crate) fn initialise(
		stream: &Object, source_object: &Object, source: &UnderlyingSource, high_water_mark: f64,
	) -> Result<ByteStreamController> {
		if let Some(auto_allocate_chunk_size) = source.auto_allocate_chunk_size
			&& auto_allocate_chunk_size == 0
		{
			return Err(Error::new("autoAllocateChunkSize can not be zero.", ErrorKind::Type));
		}

		Ok(ByteStreamController {
			common: CommonController::new_from_script(stream, Some(source_object), source, high_water_mark),
			auto_allocate_chunk_size: source.auto_allocate_chunk_size.unwrap_or(0) as usize,
			byob_request: None,
			pending_descriptors: VecDeque::new(),
			queue: VecDeque::new(),
		})
	}

	pub(crate) fn fill_pull_into_descriptor(
		&mut self, cx: &Context, descriptor: &mut PullIntoDescriptor,
	) -> Result<bool> {
		let max_copy = self.common.queue_size.min(descriptor.length - descriptor.filled);
		let max_aligned = descriptor.filled + max_copy - (descriptor.filled + max_copy) % descriptor.element;

		let ready = max_aligned > descriptor.min;

		let mut remaining = if ready {
			max_aligned - descriptor.filled
		} else {
			max_copy
		};

		while remaining > 0 {
			let mut copy = remaining;
			let mut len = 0;

			if let Some((chunk, offset, length)) = self.queue.front() {
				copy = copy.min(*length);
				len = *length;
				let chunk = ArrayBuffer::from(unsafe { Local::from_heap(chunk) }).unwrap();
				let buffer = descriptor.buffer();
				if !chunk.copy_data_to(cx, &buffer, *offset, descriptor.offset + descriptor.filled, copy) {
					let error = if let Some(Exception::Error(error)) = Exception::new(cx)? {
						error
					} else {
						Error::new("Failed to copy data to descriptor buffer.", None)
					};
					return Err(error);
				}
			}

			if copy == len {
				self.queue.pop_front();
			} else if let Some((_, offset, length)) = self.queue.get_mut(0) {
				*offset += copy;
				*length -= copy;
			}
			self.common.queue_size -= copy;
			descriptor.filled += copy;
			remaining -= copy;
		}

		if !ready {
			// TODO: Assert Queue Size 0, Assert Filled > 0, Assert Filled < Element Size
		}

		Ok(ready)
	}

	pub(crate) fn invalidate_byob_request(&mut self, cx: &Context) -> Result<()> {
		if let Some(request) = self.byob_request.take() {
			let request = Object::from(unsafe { Local::from_heap(&request) });
			let request = ByobRequest::get_mut_private(cx, &request)?;
			request.controller = None;
			request.view = None;
		}
		Ok(())
	}

	pub(crate) fn enqueue_cloned_chunk(
		&mut self, cx: &Context, buffer: &ArrayBuffer, offset: usize, length: usize,
	) -> Result<()> {
		let Some(buffer) = buffer.clone(cx, offset, length) else {
			let error = if let Some(Exception::Error(error)) = Exception::new(cx)? {
				error
			} else {
				Error::new("Failed to clone ArrayBuffer", None)
			};
			self.error_internal(cx, &error.as_value(cx))?;
			return Err(error);
		};

		self.queue.push_back((Heap::boxed(buffer.get()), 0, length));
		self.common.queue_size += length;
		Ok(())
	}

	pub(crate) fn process_descriptors(&mut self, cx: &Context, reader: &mut ByobReader, state: State) -> ResultExc<()> {
		while !self.pending_descriptors.is_empty() {
			if self.common.queue_size == 0 {
				break;
			}

			let mut shift = false;

			let descriptor = ptr::from_mut(self.pending_descriptors.get_mut(0).unwrap());
			if self.fill_pull_into_descriptor(cx, unsafe { &mut *descriptor })? {
				shift = true;
			}

			if shift {
				let mut descriptor = self.pending_descriptors.pop_front().unwrap();
				descriptor.commit(cx, reader, state)?;
			}
		}
		Ok(())
	}

	pub(crate) fn respond(&mut self, cx: &Context, written: usize) -> ResultExc<()> {
		let descriptor = self.pending_descriptors.front().unwrap();
		let stream = self.common.stream(cx)?;
		match stream.state {
			State::Readable => {
				if written == 0 {
					return Err(Error::new("Readable Stream must be written to.", ErrorKind::Type).into());
				}
				if descriptor.filled + written > descriptor.length {
					return Err(
						Error::new("Buffer of BYOB Request View has been overwritten.", ErrorKind::Range).into(),
					);
				}
			}
			State::Closed => {
				if written != 0 {
					return Err(Error::new("Closed Stream must not be written to.", ErrorKind::Type).into());
				}
			}
			State::Errored => return Err(Error::new("Errored Stream cannot have BYOB Request", ErrorKind::Type).into()),
		}

		let (buffer, kind) = {
			let descriptor = self.pending_descriptors.get_mut(0).unwrap();
			let buffer = descriptor.buffer().transfer(cx)?;
			descriptor.buffer.set(buffer.get());
			(buffer, descriptor.kind)
		};

		self.invalidate_byob_request(cx)?;

		match stream.state {
			State::Readable => {
				let descriptor = self.pending_descriptors.front_mut().unwrap();
				descriptor.filled += written;

				let PullIntoDescriptor { filled, offset, length, min, .. } = *descriptor;

				if let ReaderKind::None = kind {
					if filled > 0 {
						self.enqueue_cloned_chunk(cx, &buffer, offset, length)?;
					}

					if let Some(Reader::Byob(reader)) = stream.native_reader(cx)? {
						self.process_descriptors(cx, reader, stream.state)?;
					}
				} else {
					if filled < min {
						return Ok(());
					}

					let mut descriptor = self.pending_descriptors.pop_front().unwrap();
					let remainder = descriptor.filled % descriptor.element;

					if remainder > 0 {
						self.enqueue_cloned_chunk(
							cx,
							&buffer,
							descriptor.offset + descriptor.filled - remainder,
							remainder,
						)?;

						descriptor.filled -= remainder;
					}

					if let Some(Reader::Byob(reader)) = stream.native_reader(cx)? {
						descriptor.commit(cx, reader, stream.state)?;
						self.process_descriptors(cx, reader, stream.state)?;
					}
				}
			}
			State::Closed => match stream.native_reader(cx)? {
				Some(Reader::Byob(reader)) => {
					while !reader.common.requests.is_empty() {
						let mut descriptor = self.pending_descriptors.pop_front().unwrap();
						descriptor.commit(cx, reader, State::Closed)?;
					}
				}
				_ => {
					self.pending_descriptors.pop_front();
				}
			},
			State::Errored => unreachable!(),
		}

		self.pull_if_needed(cx)
	}

	pub(crate) fn respond_with_new_view(&mut self, cx: &Context, view: ArrayBufferView) -> ResultExc<()> {
		let buffer = view.buffer(cx);

		if buffer.is_detached() {
			return Err(Error::new("View Buffer cannot be detached.", ErrorKind::Type).into());
		}

		let stream = self.common.stream(cx)?;
		match stream.state {
			State::Readable => {
				if view.is_empty() {
					return Err(Error::new("View must have a non-zero length", ErrorKind::Type).into());
				}
			}
			State::Closed => {
				if !view.is_empty() {
					return Err(Error::new(
						"View for a Closed Readable Stream must have a zero length",
						ErrorKind::Type,
					)
					.into());
				}
			}
			State::Errored => unreachable!(),
		}

		let offset = view.offset();
		let descriptor = self.pending_descriptors.get_mut(0).unwrap();

		if descriptor.offset + descriptor.filled != offset {
			return Err(Error::new("View Offset must be the same as descriptor.", ErrorKind::Range).into());
		}
		if descriptor.length != view.len() {
			return Err(Error::new("View Length must be the same as descriptor.", ErrorKind::Range).into());
		}
		if descriptor.filled + view.len() > descriptor.length {
			return Err(Error::new("View cannot overfill descriptor", ErrorKind::Range).into());
		}

		let len = view.len();
		let buffer = buffer.transfer(cx)?;
		descriptor.buffer.set(buffer.get());
		self.respond(cx, len)
	}

	#[ion(get)]
	pub fn get_desired_size(&self, cx: &Context) -> Result<JSVal> {
		self.common.desired_size(cx)
	}

	#[ion(get)]
	pub fn get_byob_request(&mut self, cx: &Context) -> *mut JSObject {
		if self.byob_request.is_none() && !self.pending_descriptors.is_empty() {
			let descriptor = self.pending_descriptors.front().unwrap();
			let view = Uint8Array::with_array_buffer(
				cx,
				&descriptor.buffer(),
				descriptor.offset + descriptor.filled,
				descriptor.length - descriptor.filled,
			)
			.unwrap();

			let request = ByobRequest {
				reflector: Reflector::default(),
				controller: Some(Heap::boxed(self.reflector().get())),
				view: Some(Heap::boxed(view.get())),
			};
			self.byob_request = Some(Heap::boxed(ByobRequest::new_object(cx, Box::new(request))));
		}

		if let Some(request) = &self.byob_request {
			request.get()
		} else {
			ptr::null_mut()
		}
	}

	pub fn close(&mut self, cx: &Context) -> ResultExc<()> {
		let stream = self.common.stream(cx)?;
		if self.common.can_close_or_enqueue(stream) {
			if self.common.queue_size > 0 {
				self.common.close_requested = true;
			}

			if let Some(descriptor) = self.pending_descriptors.front()
				&& descriptor.filled % descriptor.element > 0
			{
				let error = Error::new("Pending Pull-Into Not Empty", ErrorKind::Type);
				self.error_internal(cx, &error.as_value(cx))?;
				return Err(error.into());
			}

			self.common.source.clear_algorithms();
			stream.close(cx)
		} else {
			Err(Error::new("Cannot Close Byte Stream Controller", ErrorKind::Type).into())
		}
	}

	pub fn enqueue(&mut self, cx: &Context, chunk: ArrayBufferView) -> ResultExc<()> {
		if chunk.is_empty() {
			return Err(Error::new("Chunk must contain bytes.", ErrorKind::Type).into());
		}

		let buffer = chunk.buffer(cx);
		if buffer.is_empty() {
			return Err(Error::new("Chunk must contain bytes.", ErrorKind::Type).into());
		}

		let stream = self.common.stream(cx)?;
		if self.common.can_close_or_enqueue(stream) {
			let offset = chunk.offset();
			let length = chunk.len();
			let buffer = buffer.transfer(cx)?;

			let mut shift = false;
			if !self.pending_descriptors.is_empty() {
				self.invalidate_byob_request(cx)?;
				let descriptor = self.pending_descriptors.front().unwrap();
				let buffer = descriptor.buffer().transfer(cx)?;
				descriptor.buffer.set(buffer.get());
				if descriptor.kind == ReaderKind::None {
					if descriptor.filled > 0 {
						self.enqueue_cloned_chunk(cx, &buffer, descriptor.offset, descriptor.length)?;
					}
					shift = true;
				}
			}

			if shift {
				self.pending_descriptors.pop_front();
			}

			match stream.native_reader(cx)? {
				Some(Reader::Default(reader)) => {
					let mut complete = false;
					while let Some(request) = reader.common.requests.pop_front() {
						let promise = request.promise();

						if self.common.queue_size == 0 {
							self.pending_descriptors.pop_front();

							let array =
								Uint8Array::with_array_buffer(cx, &buffer, offset, length).unwrap().as_value(cx);
							(request.chunk)(cx, &promise, &array);

							complete = true;
							break;
						}

						let (buffer, offset, length) = self.queue.pop_front().unwrap();
						self.common.queue_size -= length;

						if self.common.queue_size == 0 && self.common.close_requested {
							self.close(cx)?;
						} else {
							self.pull_if_needed(cx)?;
						}

						let buffer = ArrayBuffer::from(unsafe { Local::from_heap(&buffer) }).unwrap();
						let array = Uint8Array::with_array_buffer(cx, &buffer, offset, length).unwrap().as_value(cx);

						(request.chunk)(cx, &promise, &array);
					}

					if !complete {
						self.queue.push_back((Heap::boxed(buffer.get()), offset, length));
						self.common.queue_size += length;
					}
				}
				Some(Reader::Byob(reader)) => {
					self.queue.push_back((Heap::boxed(buffer.get()), offset, length));
					self.common.queue_size += length;

					self.process_descriptors(cx, reader, stream.state)?;
				}
				None => {
					self.queue.push_back((Heap::boxed(buffer.get()), offset, length));
					self.common.queue_size += length;
				}
			}
			self.pull_if_needed(cx)
		} else {
			Err(Error::new("Cannot Enqueue to Stream", ErrorKind::Type).into())
		}
	}

	pub fn error(&mut self, cx: &Context, error: Option<Value>) -> Result<()> {
		self.error_internal(cx, &error.unwrap_or_else(Value::undefined_handle))
	}
}

impl ControllerInternals for ByteStreamController {
	fn common(&mut self) -> &mut CommonController {
		&mut self.common
	}

	fn reset_queue(&mut self, cx: &Context) {
		self.invalidate_byob_request(cx).unwrap();
		self.pending_descriptors.clear();
		self.queue.clear();
		self.common.queue_size = 0;
	}
}

#[js_class]
#[ion(name = "ReadableStreamBYOBRequest")]
pub struct ByobRequest {
	reflector: Reflector,
	pub(crate) controller: Option<Box<Heap<*mut JSObject>>>,
	pub(crate) view: Option<Box<Heap<*mut JSObject>>>,
}

#[js_class]
impl ByobRequest {
	#[ion(get)]
	pub fn get_view(&self) -> *mut JSObject {
		self.view.as_ref().map_or_else(ptr::null_mut, |view| view.get())
	}

	pub fn respond(
		&mut self, cx: &Context, #[ion(convert = ConversionBehavior::Clamp
	)]
		written: u64,
	) -> ResultExc<()> {
		if let Some(controller) = self.controller.take() {
			let view = unsafe { Local::from_heap(self.view.as_ref().unwrap()) };
			let view = ArrayBufferView::from(view).unwrap();
			let buffer = view.buffer(cx);

			if view.is_empty() || buffer.is_empty() {
				return Err(Error::new("View and Buffer must have a non-zero length.", ErrorKind::Type).into());
			}

			if buffer.is_detached() {
				return Err(Error::new("View Buffer cannot be detached.", ErrorKind::Type).into());
			}

			let controller = Object::from(unsafe { Local::from_heap(&controller) });
			let controller = ByteStreamController::get_mut_private(cx, &controller)?;
			controller.respond(cx, written as usize)
		} else {
			Err(Error::new("BYOB Request has already been invalidated.", ErrorKind::Type).into())
		}
	}

	#[ion(name = "respondWithNewView")]
	pub fn respond_with_new_view(&mut self, cx: &Context, view: ArrayBufferView) -> ResultExc<()> {
		if let Some(controller) = self.controller.take() {
			let controller = Object::from(unsafe { Local::from_heap(&controller) });
			let controller = ByteStreamController::get_mut_private(cx, &controller)?;
			controller.respond_with_new_view(cx, view)
		} else {
			Err(Error::new("BYOB Request has already been invalidated.", ErrorKind::Type).into())
		}
	}
}
