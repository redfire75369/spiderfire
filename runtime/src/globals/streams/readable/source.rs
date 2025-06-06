/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::cell::Cell;
use std::rc::Rc;

use bytes::{Buf, Bytes};
use ion::class::NativeObject as _;
use ion::clone::StructuredCloneBuffer;
use ion::conversions::{FromValue as _, ToValue as _};
use ion::function::Opt;
use ion::typedarray::{ArrayBuffer, ArrayBufferView, Uint8Array};
use ion::{
	ClassDefinition as _, Context, Exception, Function, JSIterator, Local, Object, Promise, Result, ResultExc,
	Traceable, TracedHeap, Value,
};
use mozjs::gc::HandleObject;
use mozjs::jsapi::{CloneDataPolicy, Heap, JSFunction, JSObject, StructuredCloneScope};
use mozjs::jsval::{JSVal, UndefinedValue};

use crate::globals::clone::{STRUCTURED_CLONE_CALLBACKS, StructuredCloneDataHolder};
use crate::globals::streams::readable::controller::ControllerInternals as _;
use crate::globals::streams::readable::reader::{ReaderKind, Request};
use crate::globals::streams::readable::{ByobRequest, ByteStreamController, ReadableStream, ReaderOptions};

#[derive(Traceable)]
pub enum StreamSource {
	None,
	Script {
		object: Box<Heap<*mut JSObject>>,
		pull: Option<Box<Heap<*mut JSFunction>>>,
		cancel: Option<Box<Heap<*mut JSFunction>>>,
	},
	Bytes(#[trace(no_trace)] Option<Bytes>),
	BytesBuf(#[trace(no_trace)] Option<Box<dyn Buf>>),
	Iterator(#[trace(no_trace)] Box<dyn JSIterator>, Option<Box<Heap<JSVal>>>),
	TeeDefault(Rc<TeeDefaultState>, bool),
	TeeBytes(Rc<TeeBytesState>, bool),
}

impl StreamSource {
	pub fn source_object(&self) -> Object {
		match self {
			StreamSource::Script { object, .. } => Object::from(unsafe { Local::from_heap(object) }),
			_ => Object::from(Local::from_handle(HandleObject::null())),
		}
	}

	pub fn pull<'cx>(&mut self, cx: &'cx Context, controller: *mut JSObject) -> ResultExc<Option<Promise<'cx>>> {
		match self {
			StreamSource::Script { object, pull: Some(pull), .. } => {
				let pull = Function::from(unsafe { Local::from_heap(pull) });
				let controller = controller.as_value(cx);
				let this = Object::from(unsafe { Local::from_heap(object) });

				let result = pull.call(cx, &this, &[controller]).map_err(|report| report.unwrap().exception)?;
				Ok(Some(
					Promise::from_value(cx, &result, true, ()).unwrap_or_else(|_| Promise::new(cx)),
				))
			}
			StreamSource::Script { pull: None, .. } => Ok(Some(Promise::resolved(cx, &Value::undefined_handle()))),
			StreamSource::Bytes(bytes) => Ok(bytes.take().map(|bytes| {
				let buffer = ArrayBuffer::copy_from_bytes(cx, &bytes).unwrap();
				Promise::resolved(cx, &buffer.as_value(cx))
			})),
			StreamSource::BytesBuf(Some(buf)) => {
				if !buf.has_remaining() {
					return Ok(None);
				}

				let chunk = buf.chunk();
				let buffer = ArrayBuffer::copy_from_bytes(cx, chunk).unwrap();
				buf.advance(chunk.len());
				Ok(Some(Promise::resolved(cx, &buffer.as_value(cx))))
			}
			StreamSource::Iterator(iterator, Some(data)) => {
				let data = Value::from(unsafe { Local::from_heap(data) });
				Ok(iterator.next_value(cx, &data).map(|value| Promise::resolved(cx, &value)))
			}
			StreamSource::TeeDefault(state, second) => {
				if state.common.reading.get() {
					state.read_again.set(true);
					return Ok(Some(Promise::resolved(cx, &Value::undefined_handle())));
				}

				state.common.reading.set(true);

				let state1 = Rc::clone(state);
				let state2 = Rc::clone(state);
				let state3 = Rc::clone(state);
				let second = *second;

				let promise = Promise::new(cx);
				let request = Request {
					promise: Heap::boxed(promise.get()),
					chunk: Box::new(move |cx, _, chunk| {
						let promise = Promise::resolved(cx, &Value::undefined_handle());
						let chunk = TracedHeap::new(chunk.get());
						let state = Rc::clone(&state1);

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
								let result =
									buffer.write(cx, &chunk, None, &policy).and_then(|_| buffer.read(cx, &policy));

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
					}),
					close: Box::new(move |cx, _, _| {
						state2.common.reading.set(false);

						if !state2.common.cancelled[0].get() {
							let branch = state2.common.branch(cx, false)?;
							branch.native_controller(cx)?.into_default().unwrap().close(cx)?;
						}
						if !state2.common.cancelled[1].get() {
							let branch = state2.common.branch(cx, true)?;
							branch.native_controller(cx)?.into_default().unwrap().close(cx)?;
						}

						state2.common.cancel(cx, &Value::undefined_handle());
						Ok(())
					}),
					error: Box::new(move |_, _, _| {
						state3.common.reading.set(false);
					}),
				};

				let reader = state.common.stream(cx)?.native_reader(cx)?.unwrap().into_default().unwrap();
				reader.read_internal(cx, request)?;

				promise.resolve(cx, &Value::undefined_handle());
				Ok(Some(promise))
			}
			StreamSource::TeeBytes(state, second) => {
				if state.common.reading.get() {
					state.read_again[usize::from(*second)].set(true);
					return Ok(Some(Promise::resolved(cx, &Value::undefined_handle())));
				}

				state.common.reading.set(true);

				let stream = state.common.stream(cx)?;
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
						forward_reader_error(cx, &reader.common.closed(), Rc::clone(state))?;
					}

					let state1 = Rc::clone(state);
					let state2 = Rc::clone(state);
					let state3 = Rc::clone(state);

					let request = Request {
						promise: Heap::boxed(promise.get()),
						chunk: Box::new(move |cx, _, chunk| {
							let promise = Promise::resolved(cx, &Value::undefined_handle());
							let chunk = TracedHeap::new(chunk.get());
							let state = Rc::clone(&state1);

							promise.then(cx, move |cx, _| {
								state.read_again[0].set(false);
								state.read_again[1].set(false);

								let chunk = Value::from(chunk.to_local());
								let chunk = ArrayBufferView::from_value(cx, &chunk, true, ())?;
								let controller1 =
									state.common.branch(cx, false)?.native_controller(cx)?.into_byte_stream().unwrap();
								let controller2 =
									state.common.branch(cx, true)?.native_controller(cx)?.into_byte_stream().unwrap();

								match (state.common.cancelled[0].get(), state.common.cancelled[1].get()) {
									(false, false) => {
										let chunk2 = chunk
											.buffer(cx)
											.clone(cx, chunk.offset(), chunk.len())
											.and_then(|buffer| {
												Uint8Array::with_array_buffer(cx, &buffer, 0, buffer.len())
											})
											.map(|array| ArrayBufferView::from(array.into_local()).unwrap());

										if let Some(chunk2) = chunk2 {
											controller1.enqueue(cx, chunk)?;
											controller2.enqueue(cx, chunk2)?;
										} else {
											let exception = Exception::new(cx).unwrap().as_value(cx);
											controller1.error_internal(cx, &exception)?;
											controller2.error_internal(cx, &exception)?;
											state.common.cancel(cx, &Value::undefined_handle());
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
						}),
						close: Box::new(move |cx, _, _| {
							state2.common.reading.set(false);

							let controller1 =
								state2.common.branch(cx, false)?.native_controller(cx)?.into_byte_stream().unwrap();
							let controller2 =
								state2.common.branch(cx, true)?.native_controller(cx)?.into_byte_stream().unwrap();

							if !state2.common.cancelled[0].get() {
								controller1.close(cx)?;
							}
							if !state2.common.cancelled[1].get() {
								controller2.close(cx)?;
							}

							if !controller1.pending_descriptors.is_empty() {
								controller1.respond(cx, 0)?;
							}
							if !controller2.pending_descriptors.is_empty() {
								controller2.respond(cx, 0)?;
							}

							state2.common.cancel(cx, &Value::undefined_handle());
							Ok(())
						}),
						error: Box::new(move |_, _, _| {
							state3.common.reading.set(false);
						}),
					};

					let reader = state.common.stream(cx)?.native_reader(cx)?.unwrap().into_default().unwrap();
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
						forward_reader_error(cx, &reader.common.closed(), Rc::clone(state))?;
					}

					let state1 = Rc::clone(state);
					let state2 = Rc::clone(state);
					let state3 = Rc::clone(state);
					let second = *second;

					let request = Request {
						promise: Heap::boxed(promise.get()),
						chunk: Box::new(move |cx, _, chunk| {
							let promise = Promise::resolved(cx, &Value::undefined_handle());
							let chunk = TracedHeap::new(chunk.get());
							let state = Rc::clone(&state1);

							promise.then(cx, move |cx, _| {
								state.read_again[0].set(false);
								state.read_again[1].set(false);

								let chunk = Value::from(chunk.to_local());
								let chunk = ArrayBufferView::from_value(cx, &chunk, true, ())?;

								let byob_controller =
									state.common.branch(cx, second)?.native_controller(cx)?.into_byte_stream().unwrap();
								let other_controller = state
									.common
									.branch(cx, !second)?
									.native_controller(cx)?
									.into_byte_stream()
									.unwrap();

								if !state.common.cancelled[usize::from(!second)].get() {
									let chunk2 = chunk
										.buffer(cx)
										.clone(cx, chunk.offset(), chunk.len())
										.and_then(|buffer| Uint8Array::with_array_buffer(cx, &buffer, 0, buffer.len()))
										.map(|array| ArrayBufferView::from(array.into_local()).unwrap());

									if let Some(chunk2) = chunk2 {
										other_controller.enqueue(cx, chunk2)?;
									} else {
										let exception = Exception::new(cx).unwrap().as_value(cx);
										byob_controller.error_internal(cx, &exception)?;
										other_controller.error_internal(cx, &exception)?;
										state.common.cancel(cx, &Value::undefined_handle());
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
						}),
						close: Box::new(move |cx, _, _| {
							state2.common.reading.set(false);

							let controller1 =
								state2.common.branch(cx, false)?.native_controller(cx)?.into_byte_stream().unwrap();
							let controller2 =
								state2.common.branch(cx, true)?.native_controller(cx)?.into_byte_stream().unwrap();

							if !state2.common.cancelled[0].get() {
								controller1.close(cx)?;
							}
							if !state2.common.cancelled[1].get() {
								controller2.close(cx)?;
							}

							if !controller1.pending_descriptors.is_empty() {
								controller1.respond(cx, 0)?;
							}
							if !controller2.pending_descriptors.is_empty() {
								controller2.respond(cx, 0)?;
							}

							state2.common.cancel(cx, &Value::undefined_handle());
							Ok(())
						}),
						error: Box::new(move |_, _, _| {
							state3.common.reading.set(false);
						}),
					};

					let reader = state.common.stream(cx)?.native_reader(cx)?.unwrap().into_byob().unwrap();
					let byob_request = Object::from(cx.root(byob_request));
					let byob_request = ByobRequest::get_private(cx, &byob_request)?;

					let view = ArrayBufferView::from(cx.root(byob_request.get_view())).unwrap();
					reader.read_internal(cx, view, 1, request)?;
				}

				promise.resolve(cx, &Value::undefined_handle());
				Ok(Some(promise))
			}
			_ => Ok(None),
		}
	}

	pub fn cancel(&mut self, cx: &Context, promise: &mut Promise, reason: Option<Value>) -> ResultExc<()> {
		match self {
			StreamSource::Script { object, cancel: Some(cancel), .. } => {
				let cancel = Function::from(unsafe { Local::from_heap(cancel) });
				let this = Object::from(unsafe { Local::from_heap(object) });
				let reason = reason.unwrap_or_else(Value::undefined_handle);

				let result = cancel.call(cx, &this, &[reason]).map_err(|report| report.unwrap().exception)?;
				if let Ok(result) = Promise::from_value(cx, &result, true, ()) {
					result.then(cx, |_, _| Ok(Value::undefined_handle()));
					promise.handle_mut().set(result.get());
				} else {
					promise.resolve(cx, &Value::undefined_handle());
				}
			}
			StreamSource::TeeDefault(state, second) => {
				let reason = reason.unwrap_or_else(Value::undefined_handle);

				let branch = usize::from(*second);
				state.common.cancelled[branch].set(true);
				state.common.reason[branch].set(reason.get());

				if state.common.cancelled[usize::from(!*second)].get() {
					let composite = [state.common.reason[0].get(), state.common.reason[1].get()].as_value(cx);
					let result = state.common.stream(cx)?.cancel(cx, Opt(Some(composite)))?;

					let cancel_promise = state.common.cancel_promise();
					cancel_promise.resolve(cx, &result.as_value(cx));
				}

				promise.handle_mut().set(state.common.cancel_promise.get());
			}
			_ => {}
		}

		Ok(())
	}

	pub fn clear_algorithms(&mut self) {
		match self {
			StreamSource::Script { pull, cancel, .. } => {
				*pull = None;
				*cancel = None;
			}
			StreamSource::Bytes(bytes) => {
				*bytes = None;
			}
			StreamSource::BytesBuf(buf) => {
				*buf = None;
			}
			StreamSource::Iterator(_, data) => {
				*data = None;
			}
			_ => {}
		}
	}
}

#[derive(Traceable)]
pub(crate) struct TeeCommonState {
	stream: Box<Heap<*mut JSObject>>,
	pub(crate) branch: [Box<Heap<*mut JSObject>>; 2],

	reading: Cell<bool>,
	cancelled: [Cell<bool>; 2],

	reason: [Box<Heap<JSVal>>; 2],
	cancel_promise: Box<Heap<*mut JSObject>>,
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

	pub(crate) fn stream(&self, cx: &Context) -> Result<&mut ReadableStream> {
		let stream = Object::from(unsafe { Local::from_heap(&self.stream) });
		ReadableStream::get_mut_private(cx, &stream)
	}

	pub(crate) fn branch(&self, cx: &Context, second: bool) -> Result<&mut ReadableStream> {
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
	clone_branch_2: bool,
	read_again: Cell<bool>,
}

impl TeeDefaultState {
	pub(crate) fn new(cx: &Context, stream: &ReadableStream, clone_branch_2: bool) -> TeeDefaultState {
		TeeDefaultState {
			common: TeeCommonState::new(cx, stream),
			clone_branch_2,
			read_again: Cell::new(false),
		}
	}
}

#[derive(Traceable)]
pub struct TeeBytesState {
	pub(crate) common: TeeCommonState,
	read_again: [Cell<bool>; 2],
}

impl TeeBytesState {
	pub(crate) fn new(cx: &Context, stream: &ReadableStream) -> TeeBytesState {
		TeeBytesState {
			common: TeeCommonState::new(cx, stream),
			read_again: [Cell::new(false), Cell::new(false)],
		}
	}
}

pub(crate) fn forward_reader_error(cx: &Context, closed_promise: &Promise, state: Rc<TeeBytesState>) -> Result<()> {
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
