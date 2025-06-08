/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::rc::Rc;

use bytes::{Buf, Bytes};
use ion::conversions::{FromValue as _, ToValue as _};
use ion::function::Opt;
use ion::typedarray::ArrayBuffer;
use ion::{Context, Function, JSIterator, Local, Object, Promise, ResultExc, Traceable, Value};
use mozjs::gc::HandleObject;
use mozjs::jsapi::{Heap, JSFunction, JSObject};
use mozjs::jsval::JSVal;

use crate::globals::streams::readable::tee::{TeeBytesState, TeeDefaultState};

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
			StreamSource::TeeDefault(state, second) => state.pull(cx, *second),
			StreamSource::TeeBytes(state, second) => state.pull(cx, *second),
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
