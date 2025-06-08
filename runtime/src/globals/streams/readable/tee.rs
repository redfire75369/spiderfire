/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::cell::Cell;
use std::rc::Rc;

use ion::class::NativeObject as _;
use ion::clone::StructuredCloneBuffer;
use ion::conversions::{FromValue as _, ToValue as _};
use ion::function::Opt;
use ion::typedarray::{ArrayBufferView, Uint8Array};
use ion::{ClassDefinition as _, Context, Exception, Local, Object, Promise, ResultExc, Traceable, TracedHeap, Value};
use mozjs::jsapi::{CloneDataPolicy, Heap, JSObject, StructuredCloneScope};
use mozjs::jsval::{JSVal, UndefinedValue};

use crate::globals::clone::{STRUCTURED_CLONE_CALLBACKS, StructuredCloneDataHolder};
use crate::globals::streams::readable::controller::ControllerInternals as _;
use crate::globals::streams::readable::reader::{ChunkErrorClosure, CloseClosure, ReaderKind, Request};
use crate::globals::streams::readable::{ByobRequest, ByteStreamController, ReadableStream, ReaderOptions};

#[derive(Traceable)]
pub(crate) struct TeeCommonState {
	stream: Box<Heap<*mut JSObject>>,
	pub(crate) branch: [Box<Heap<*mut JSObject>>; 2],

	pub(crate) reading: Cell<bool>,
	pub(crate) cancelled: [Cell<bool>; 2],

	pub(crate) reason: [Box<Heap<JSVal>>; 2],
	pub(crate) cancel_promise: Box<Heap<*mut JSObject>>,
}

impl TeeCommonState {
	pub(crate) fn new(cx: &Context, stream: &ReadableStream) -> TeeCommonState {
		let promise = Promise::new(cx);
		TeeCommonState {
			stream: Heap::boxed(stream.reflector.get()),
			branch: [Box::default(), Box::default()],

			reading: Cell::new(false),
			cancelled: [Cell::new(false), Cell::new(false)],

			reason: [Heap::boxed(UndefinedValue()), Heap::boxed(UndefinedValue())],
			cancel_promise: Heap::boxed(promise.get()),
		}
	}

	pub(crate) fn stream(&self, cx: &Context) -> ion::Result<&mut ReadableStream> {
		let stream = Object::from(unsafe { Local::from_heap(&self.stream) });
		ReadableStream::get_mut_private(cx, &stream)
	}

	pub(crate) fn branch(&self, cx: &Context, second: bool) -> ion::Result<&mut ReadableStream> {
		let stream = Object::from(unsafe { Local::from_heap(&self.branch[usize::from(second)]) });
		ReadableStream::get_mut_private(cx, &stream)
	}

	pub(crate) fn cancel_promise(&self) -> Promise {
		Promise::from(unsafe { Local::from_heap(&self.cancel_promise) }).unwrap()
	}

	pub(crate) fn cancel(&self, cx: &Context, value: &Value) {
		if !self.cancelled[0].get() || !self.cancelled[1].get() {
			self.cancel_promise().resolve(cx, value);
		}
	}
}

#[derive(Traceable)]
pub struct TeeDefaultState {
	pub(crate) common: TeeCommonState,
	pub(crate) clone_branch_2: bool,
	pub(crate) read_again: Cell<bool>,
}

impl TeeDefaultState {
	pub(crate) fn new(cx: &Context, stream: &ReadableStream, clone_branch_2: bool) -> TeeDefaultState {
		TeeDefaultState {
			common: TeeCommonState::new(cx, stream),
			clone_branch_2,
			read_again: Cell::new(false),
		}
	}

	pub(crate) fn pull<'cx>(self: &Rc<Self>, cx: &'cx Context, second: bool) -> ResultExc<Option<Promise<'cx>>> {
		if self.common.reading.get() {
			self.read_again.set(true);
			return Ok(Some(Promise::resolved(cx, &Value::undefined_handle())));
		}

		self.common.reading.set(true);

		let promise = Promise::new(cx);
		let request = Request {
			promise: Heap::boxed(promise.get()),
			chunk: Rc::clone(self).chunk_closure(second),
			close: Rc::clone(self).close_closure(),
			error: error_closure(Rc::clone(self)),
		};

		let reader = self.common.stream(cx)?.native_reader(cx)?.unwrap().into_default().unwrap();
		reader.read_internal(cx, request)?;

		promise.resolve(cx, &Value::undefined_handle());
		Ok(Some(promise))
	}

	fn chunk_closure(self: Rc<Self>, second: bool) -> Box<ChunkErrorClosure> {
		Box::new(move |cx, _, chunk| {
			let promise = Promise::resolved(cx, &Value::undefined_handle());
			let chunk = TracedHeap::new(chunk.get());

			let state = Rc::clone(&self);
			promise.then(cx, move |cx, _| {
				state.read_again.set(false);
				let chunk = Value::from(chunk.to_local());
				let mut chunk2 = None;

				if !state.common.cancelled[1].get() && state.clone_branch_2 {
					let policy = CloneDataPolicy {
						allowIntraClusterClonableSharedObjects_: false,
						allowSharedMemoryObjects_: true,
					};

					let mut buffer = StructuredCloneBuffer::new(
						StructuredCloneScope::SameProcess,
						&STRUCTURED_CLONE_CALLBACKS,
						Some(Box::new(StructuredCloneDataHolder::default())),
					);
					let result = buffer.write(cx, &chunk, None, &policy).and_then(|_| buffer.read(cx, &policy));

					match result {
						Ok(chunk) => {
							chunk2 = Some(chunk);
						}
						Err(e) => {
							let value = e.as_value(cx);
							let branch1 = state.common.branch(cx, false)?;
							let controller1 = branch1.native_controller(cx)?.into_default().unwrap();
							controller1.error_internal(cx, &value)?;

							let branch2 = state.common.branch(cx, true)?;
							let controller2 = branch2.native_controller(cx)?.into_default().unwrap();
							controller2.error_internal(cx, &value)?;

							state.common.cancel(cx, &value);
							return Ok(Value::undefined_handle());
						}
					}
				}

				if !state.common.cancelled[0].get() {
					let branch = state.common.branch(cx, false)?;
					let controller = branch.native_controller(cx)?.into_default().unwrap();
					controller.enqueue_internal(cx, &chunk)?;
				}
				if !state.common.cancelled[1].get() {
					let branch = state.common.branch(cx, true)?;
					let controller = branch.native_controller(cx)?.into_default().unwrap();
					controller.enqueue_internal(cx, chunk2.as_ref().unwrap_or(&chunk))?;
				}

				state.common.reading.set(false);
				if state.read_again.get() {
					let branch = state.common.branch(cx, second)?;
					let controller = branch.native_controller(cx)?.into_default().unwrap();
					controller.common.source.pull(cx, controller.reflector().get())?;
				}
				Ok(Value::undefined_handle())
			});
		})
	}

	fn close_closure(self: Rc<Self>) -> Box<CloseClosure> {
		Box::new(move |cx, _, _| {
			self.common.reading.set(false);

			if !self.common.cancelled[0].get() {
				let branch = self.common.branch(cx, false)?;
				branch.native_controller(cx)?.into_default().unwrap().close(cx)?;
			}
			if !self.common.cancelled[1].get() {
				let branch = self.common.branch(cx, true)?;
				branch.native_controller(cx)?.into_default().unwrap().close(cx)?;
			}

			self.common.cancel(cx, &Value::undefined_handle());
			Ok(())
		})
	}
}

#[derive(Traceable)]
pub struct TeeBytesState {
	pub(crate) common: TeeCommonState,
	pub(crate) read_again: [Cell<bool>; 2],
}

impl TeeBytesState {
	pub(crate) fn new(cx: &Context, stream: &ReadableStream) -> TeeBytesState {
		TeeBytesState {
			common: TeeCommonState::new(cx, stream),
			read_again: [Cell::new(false), Cell::new(false)],
		}
	}

	pub(crate) fn pull<'cx>(self: &Rc<Self>, cx: &'cx Context, second: bool) -> ResultExc<Option<Promise<'cx>>> {
		if self.common.reading.get() {
			self.read_again[usize::from(second)].set(true);
			return Ok(Some(Promise::resolved(cx, &Value::undefined_handle())));
		}

		self.common.reading.set(true);

		let stream = self.common.stream(cx)?;
		let controller = stream.native_controller(cx)?.into_byte_stream().unwrap();
		let byob_request = controller.get_byob_request(cx);

		let promise = Promise::new(cx);
		if byob_request.is_null() {
			if stream.reader_kind == ReaderKind::Byob {
				{
					let reader = stream.native_reader(cx)?.unwrap().into_byob().unwrap();
					assert!(reader.common.requests.is_empty());
					reader.release_lock(cx)?;
				}
				stream.get_reader(cx, Opt(None))?;

				let reader = stream.native_reader(cx)?.unwrap().into_default().unwrap();
				forward_reader_error(cx, &reader.common.closed(), Rc::clone(self))?;
			}

			let request = Request {
				promise: Heap::boxed(promise.get()),
				chunk: Rc::clone(self).chunk_closure(),
				close: Rc::clone(self).close_closure(),
				error: error_closure(Rc::clone(self)),
			};

			let reader = self.common.stream(cx)?.native_reader(cx)?.unwrap().into_default().unwrap();
			reader.read_internal(cx, request)?;
		} else {
			if stream.reader_kind == ReaderKind::Default {
				{
					let reader = stream.native_reader(cx)?.unwrap().into_default().unwrap();
					assert!(reader.common.requests.is_empty());
					reader.release_lock(cx)?;
				}
				stream.get_reader(cx, Opt(Some(ReaderOptions { mode: Some(String::from("byob")) })))?;

				let reader = stream.native_reader(cx)?.unwrap().into_byob().unwrap();
				forward_reader_error(cx, &reader.common.closed(), Rc::clone(self))?;
			}

			let request = Request {
				promise: Heap::boxed(promise.get()),
				chunk: Rc::clone(self).chunk_byob_closure(second),
				close: Rc::clone(self).close_closure(),
				error: error_closure(Rc::clone(self)),
			};

			let reader = self.common.stream(cx)?.native_reader(cx)?.unwrap().into_byob().unwrap();
			let byob_request = Object::from(cx.root(byob_request));
			let byob_request = ByobRequest::get_private(cx, &byob_request)?;

			let view = ArrayBufferView::from(cx.root(byob_request.get_view())).unwrap();
			reader.read_internal(cx, view, 1, request)?;
		}

		promise.resolve(cx, &Value::undefined_handle());
		Ok(Some(promise))
	}

	fn chunk_closure(self: Rc<Self>) -> Box<ChunkErrorClosure> {
		Box::new(move |cx, _, chunk| {
			let promise = Promise::resolved(cx, &Value::undefined_handle());
			let chunk = TracedHeap::new(chunk.get());

			let state = Rc::clone(&self);
			promise.then(cx, move |cx, _| {
				state.read_again[0].set(false);
				state.read_again[1].set(false);

				let chunk = Value::from(chunk.to_local());
				let chunk = ArrayBufferView::from_value(cx, &chunk, true, ())?;
				let controller1 = state.common.branch(cx, false)?.native_controller(cx)?.into_byte_stream().unwrap();
				let controller2 = state.common.branch(cx, true)?.native_controller(cx)?.into_byte_stream().unwrap();

				match (state.common.cancelled[0].get(), state.common.cancelled[1].get()) {
					(false, false) => {
						let chunk2 = clone_bytes_chunk(cx, &chunk);

						if let Some(chunk2) = chunk2 {
							controller1.enqueue(cx, chunk)?;
							controller2.enqueue(cx, chunk2)?;
						} else {
							state.chunk_error(cx, [controller1, controller2])?;
						}

						state.common.reading.set(false);
					}
					(false, true) => controller1.enqueue(cx, chunk)?,
					(true, false) => controller2.enqueue(cx, chunk)?,
					_ => {}
				}

				state.common.reading.set(false);
				if state.read_again[0].get() {
					controller1.common.source.pull(cx, controller1.reflector().get())?;
				} else if state.read_again[1].get() {
					controller2.common.source.pull(cx, controller2.reflector().get())?;
				}

				Ok(Value::undefined_handle())
			});
		})
	}

	fn chunk_byob_closure(self: Rc<Self>, second: bool) -> Box<ChunkErrorClosure> {
		Box::new(move |cx, _, chunk| {
			let promise = Promise::resolved(cx, &Value::undefined_handle());
			let chunk = TracedHeap::new(chunk.get());

			let state = Rc::clone(&self);
			promise.then(cx, move |cx, _| {
				state.read_again[0].set(false);
				state.read_again[1].set(false);

				let chunk = Value::from(chunk.to_local());
				let chunk = ArrayBufferView::from_value(cx, &chunk, true, ())?;

				let byob_controller =
					state.common.branch(cx, second)?.native_controller(cx)?.into_byte_stream().unwrap();
				let other_controller =
					state.common.branch(cx, !second)?.native_controller(cx)?.into_byte_stream().unwrap();

				if !state.common.cancelled[usize::from(!second)].get() {
					let chunk2 = clone_bytes_chunk(cx, &chunk);

					if let Some(chunk2) = chunk2 {
						other_controller.enqueue(cx, chunk2)?;
					} else {
						state.chunk_error(cx, [byob_controller, other_controller])?;
					}
				}
				if !state.common.cancelled[usize::from(second)].get() {
					byob_controller.respond_with_new_view(cx, chunk)?;
				}

				state.common.reading.set(false);
				if state.read_again[usize::from(second)].get() {
					byob_controller.common.source.pull(cx, byob_controller.reflector().get())?;
				} else if state.read_again[usize::from(!second)].get() {
					other_controller.common.source.pull(cx, other_controller.reflector().get())?;
				}

				Ok(Value::undefined_handle())
			});
		})
	}

	fn close_closure(self: Rc<Self>) -> Box<CloseClosure> {
		Box::new(move |cx, _, _| {
			self.common.reading.set(false);

			let controller1 = self.common.branch(cx, false)?.native_controller(cx)?.into_byte_stream().unwrap();
			let controller2 = self.common.branch(cx, true)?.native_controller(cx)?.into_byte_stream().unwrap();

			if !self.common.cancelled[0].get() {
				controller1.close(cx)?;
			}
			if !self.common.cancelled[1].get() {
				controller2.close(cx)?;
			}

			if !controller1.pending_descriptors.is_empty() {
				controller1.respond(cx, 0)?;
			}
			if !controller2.pending_descriptors.is_empty() {
				controller2.respond(cx, 0)?;
			}

			self.common.cancel(cx, &Value::undefined_handle());
			Ok(())
		})
	}

	fn chunk_error(&self, cx: &Context, controllers: [&mut ByteStreamController; 2]) -> ResultExc<()> {
		let exception = Exception::new(cx).unwrap().as_value(cx);
		for controller in controllers {
			controller.error_internal(cx, &exception)?;
		}
		self.common.cancel(cx, &Value::undefined_handle());
		Ok(())
	}
}

pub(crate) trait TeeState {
	fn common(&self) -> &TeeCommonState;
}

impl TeeState for TeeDefaultState {
	fn common(&self) -> &TeeCommonState {
		&self.common
	}
}

impl TeeState for TeeBytesState {
	fn common(&self) -> &TeeCommonState {
		&self.common
	}
}

impl<T: TeeState> TeeState for Rc<T> {
	fn common(&self) -> &TeeCommonState {
		(**self).common()
	}
}

fn clone_bytes_chunk<'cx>(cx: &'cx Context, chunk: &ArrayBufferView) -> Option<ArrayBufferView<'cx>> {
	chunk
		.buffer(cx)
		.clone(cx, chunk.offset(), chunk.len())
		.and_then(|buffer| Uint8Array::with_array_buffer(cx, &buffer, 0, buffer.len()))
		.map(|array| ArrayBufferView::from(array.into_local()).unwrap())
}

fn error_closure<T: TeeState + 'static>(state: T) -> Box<ChunkErrorClosure> {
	Box::new(move |_, _, _| {
		state.common().reading.set(false);
	})
}

pub(crate) fn forward_reader_error(
	cx: &Context, closed_promise: &Promise, state: Rc<TeeBytesState>,
) -> ion::Result<()> {
	let controller1 = TracedHeap::new(state.common.branch(cx, false)?.controller.get());
	let controller2 = TracedHeap::new(state.common.branch(cx, true)?.controller.get());

	closed_promise.catch(cx, move |cx, reason| {
		ByteStreamController::from_traced_heap(cx, &controller1)?.error_internal(cx, reason)?;
		ByteStreamController::from_traced_heap(cx, &controller2)?.error_internal(cx, reason)?;
		state.common.cancel(cx, &Value::undefined_handle());
		Ok(Value::undefined_handle())
	});
	Ok(())
}
