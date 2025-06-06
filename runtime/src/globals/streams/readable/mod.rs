/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::collections::VecDeque;
use std::rc::Rc;

pub use controller::{ByobRequest, ByteStreamController, CommonController, DefaultController};
use controller::{Controller, ControllerInternals as _, ControllerKind};
use ion::class::{NativeObject as _, Reflector};
use ion::conversions::{ConversionBehavior, FromValue as _, ToValue as _};
use ion::function::Opt;
use ion::{
	ClassDefinition as _, Context, Error, ErrorKind, FromValue, Function, Local, Object, Promise, Result, ResultExc,
	Traceable, Value, js_class,
};
use mozjs::jsapi::{Heap, JSObject};
use mozjs::jsval::JSVal;
pub use reader::{ByobReader, CommonReader, DefaultReader};
use reader::{Reader, ReaderKind};
pub use source::StreamSource;
use source::{TeeBytesState, TeeDefaultState, forward_reader_error};

mod controller;
mod reader;
mod source;

#[derive(Default, FromValue)]
pub struct UnderlyingSource<'cx> {
	start: Option<Function<'cx>>,
	pull: Option<Function<'cx>>,
	cancel: Option<Function<'cx>>,
	#[ion(name = "type")]
	ty: Option<String>,
	#[ion(convert = ConversionBehavior::EnforceRange)]
	auto_allocate_chunk_size: Option<u64>,
}

impl UnderlyingSource<'_> {
	pub(crate) fn to_native(&self, object: Option<&Object>) -> StreamSource {
		match object {
			Some(object) => StreamSource::Script {
				object: Heap::boxed(object.handle().get()),
				pull: self.pull.as_ref().map(|pull| Heap::boxed(pull.get())),
				cancel: self.cancel.as_ref().map(|cancel| Heap::boxed(cancel.get())),
			},
			None => StreamSource::None,
		}
	}
}

#[derive(Default, FromValue)]
pub struct QueueingStrategy<'cx> {
	high_water_mark: Option<f64>,
	size: Option<Function<'cx>>,
}

#[derive(Default, FromValue)]
pub struct ReaderOptions {
	mode: Option<String>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Traceable)]
pub enum State {
	Readable,
	Closed,
	Errored,
}

#[js_class]
pub struct ReadableStream {
	reflector: Reflector,

	pub(crate) controller_kind: ControllerKind,
	pub(crate) controller: Box<Heap<*mut JSObject>>,

	pub(crate) reader_kind: ReaderKind,
	pub(crate) reader: Option<Box<Heap<*mut JSObject>>>,

	pub(crate) state: State,
	pub(crate) disturbed: bool,
	pub(crate) error: Option<Box<Heap<JSVal>>>,
}

#[js_class]
impl ReadableStream {
	#[ion(constructor)]
	pub fn constructor<'cx: 'o, 'o>(
		cx: &'cx Context, #[ion(this)] this: &Object, Opt(underlying_source): Opt<Object<'o>>,
		Opt(strategy): Opt<QueueingStrategy>,
	) -> ResultExc<ReadableStream> {
		let strategy = strategy.unwrap_or_default();
		let mut source = None;

		let controller = underlying_source
			.as_ref()
			.map::<ResultExc<_>, _>(|underlying_source| {
				let source_value = underlying_source.as_value(cx);
				source = Some(UnderlyingSource::from_value(cx, &source_value, false, ())?);

				let source = source.as_ref().unwrap();
				if source.ty.as_deref() == Some("bytes") {
					if strategy.size.is_some() {
						return Err(Error::new("Implementation preserved member 'size'", ErrorKind::Range).into());
					}

					if let Some(high_water_mark) = strategy.high_water_mark {
						if high_water_mark.is_nan() {
							return Err(Error::new("highWaterMark cannot be NaN", ErrorKind::Range).into());
						} else if high_water_mark < 0.0 {
							return Err(Error::new("highWaterMark must be non-negative", ErrorKind::Range).into());
						}
					}
					let high_water_mark = strategy.high_water_mark.unwrap_or(0.0);

					let controller =
						ByteStreamController::initialise(this, underlying_source, source, high_water_mark)?;
					let controller = Heap::boxed(ByteStreamController::new_object(cx, Box::new(controller)));
					unsafe {
						let controller = Object::from(Local::from_heap(&controller));
						ByteStreamController::get_mut_private_unchecked(&controller)
							.start(cx, source.start.as_ref())?;
					}

					Ok(Some((ControllerKind::ByteStream, controller)))
				} else if source.ty.is_some() {
					Err(Error::new(
						"Type of Underlying Source must be 'bytes' or not exist.",
						ErrorKind::Type,
					)
					.into())
				} else {
					Ok(None)
				}
			})
			.transpose()?
			.flatten();

		let (controller_kind, controller) = if let Some(controller) = controller {
			controller
		} else {
			let source = source.unwrap_or_default();
			let high_water_mark = strategy.high_water_mark.unwrap_or(1.0);
			let controller =
				DefaultController::initialise(this, underlying_source.as_ref(), &source, &strategy, high_water_mark);
			let controller = Heap::boxed(DefaultController::new_object(cx, Box::new(controller)));
			unsafe {
				let controller = Object::from(Local::from_heap(&controller));
				DefaultController::get_mut_private_unchecked(&controller).start(cx, source.start.as_ref())?;
			}

			(ControllerKind::Default, controller)
		};

		Ok(ReadableStream::new(controller_kind, controller))
	}

	#[ion(get)]
	pub fn get_locked(&self) -> bool {
		self.reader_kind != ReaderKind::None
	}

	pub fn cancel<'cx>(&mut self, cx: &'cx Context, Opt(reason): Opt<Value>) -> ResultExc<Promise<'cx>> {
		if self.get_locked() {
			Err(Error::new("ReadableStream is locked.", ErrorKind::Type).into())
		} else {
			self.disturbed = true;
			match self.state {
				State::Readable => {
					self.close(cx)?;
					self.native_controller(cx)?.cancel(cx, reason)
				}
				State::Closed => Ok(Promise::resolved(cx, &Value::undefined_handle())),
				State::Errored => {
					let mut value = Value::null(cx);
					if let Some(error) = &self.error {
						value.handle_mut().set(error.get());
					}
					let promise = Promise::new(cx);
					promise.reject(cx, &value);
					Ok(promise)
				}
			}
		}
	}

	#[ion(name = "getReader")]
	pub fn get_reader<'cx>(&mut self, cx: &'cx Context, Opt(options): Opt<ReaderOptions>) -> Result<Object<'cx>> {
		if self.get_locked() {
			return Err(Error::new(
				"New readers cannot be initialised for locked streams.",
				ErrorKind::Type,
			));
		}

		let options = options.unwrap_or_default();
		if let Some(mode) = &options.mode {
			if mode == "byob" {
				let reader = ByobReader::new(cx, &Object::from(cx.root(self.reflector().get())))?;
				let object = Object::from(cx.root(ByobReader::new_object(cx, Box::new(reader))));

				self.reader_kind = ReaderKind::Byob;
				self.reader = Some(Heap::boxed(object.handle().get()));

				Ok(object)
			} else {
				Err(Error::new("Mode must be 'byob' or must not exist.", ErrorKind::Type))
			}
		} else {
			let reader = DefaultReader::new(cx, &Object::from(Local::from_handle(self.reflector().handle())))?;
			let object = Object::from(cx.root(DefaultReader::new_object(cx, Box::new(reader))));

			self.reader_kind = ReaderKind::Default;
			self.reader = Some(Heap::boxed(object.handle().get()));

			Ok(object)
		}
	}

	pub fn tee<'cx>(&mut self, cx: &'cx Context) -> Result<[Object<'cx>; 2]> {
		self.get_reader(cx, Opt(None))?;
		Ok(self.tee_internal(cx, false))
	}
}

impl ReadableStream {
	pub(crate) fn new(controller_kind: ControllerKind, controller: Box<Heap<*mut JSObject>>) -> ReadableStream {
		ReadableStream {
			reflector: Reflector::default(),

			controller_kind,
			controller,

			reader_kind: ReaderKind::None,
			reader: None,

			state: State::Readable,
			disturbed: false,
			error: None,
		}
	}

	pub(crate) fn tee_internal<'cx>(&mut self, cx: &'cx Context, clone_branch_2: bool) -> [Object<'cx>; 2] {
		match self.controller_kind {
			ControllerKind::Default => {
				fn create_branch(cx: &Context, state: Rc<TeeDefaultState>, second: bool) -> Object {
					let branch = Object::from(cx.root(ReadableStream::new_raw_object(cx)));
					let source = StreamSource::TeeDefault(state, second);
					let controller = DefaultController {
						common: CommonController::new(&branch, source, 1.0),
						size: None,
						queue: VecDeque::default(),
					};
					let controller = Heap::boxed(DefaultController::new_object(cx, Box::new(controller)));

					unsafe {
						let controller = Object::from(Local::from_heap(&controller));
						DefaultController::get_mut_private_unchecked(&controller).start(cx, None).unwrap();
					}

					let stream = ReadableStream::new(ControllerKind::Default, controller);
					unsafe {
						ReadableStream::set_private(branch.handle().get(), Box::new(stream));
					}
					branch
				}

				let state = Rc::new(TeeDefaultState::new(cx, self, clone_branch_2));
				let branch1 = create_branch(cx, Rc::clone(&state), false);
				let branch2 = create_branch(cx, Rc::clone(&state), true);

				state.common.branch[0].set(branch1.handle().get());
				state.common.branch[1].set(branch2.handle().get());

				[branch1, branch2]
			}
			ControllerKind::ByteStream => {
				fn create_branch(cx: &Context, state: Rc<TeeBytesState>, second: bool) -> Object {
					let branch = Object::from(cx.root(ReadableStream::new_raw_object(cx)));
					let source = StreamSource::TeeBytes(state, second);
					let controller = ByteStreamController {
						common: CommonController::new(&branch, source, 1.0),
						auto_allocate_chunk_size: 0,
						byob_request: None,
						pending_descriptors: VecDeque::default(),
						queue: VecDeque::default(),
					};
					let controller = Heap::boxed(ByteStreamController::new_object(cx, Box::new(controller)));

					unsafe {
						let controller = Object::from(Local::from_heap(&controller));
						ByteStreamController::get_mut_private_unchecked(&controller).start(cx, None).unwrap();
					}

					let stream = ReadableStream::new(ControllerKind::ByteStream, controller);
					unsafe {
						ReadableStream::set_private(branch.handle().get(), Box::new(stream));
					}
					branch
				}

				let state = Rc::new(TeeBytesState::new(cx, self));
				let branch1 = create_branch(cx, Rc::clone(&state), false);
				let branch2 = create_branch(cx, Rc::clone(&state), true);

				state.common.branch[0].set(branch1.handle().get());
				state.common.branch[1].set(branch2.handle().get());

				let reader = self.native_reader(cx).unwrap().unwrap().into_default().unwrap();
				forward_reader_error(cx, &reader.common.closed(), Rc::clone(&state)).unwrap();

				[branch1, branch2]
			}
		}
	}

	pub(crate) fn close(&mut self, cx: &Context) -> ResultExc<()> {
		if self.state != State::Readable {
			return Err(Error::new("Cannot Close Stream", None).into());
		}

		self.state = State::Closed;
		let (requests, closed) = match self.native_reader(cx)? {
			Some(reader) => reader.requests_closed(),
			None => return Ok(()),
		};

		closed.resolve(cx, &Value::undefined_handle());
		if self.reader_kind == ReaderKind::Default {
			for request in &*requests {
				let promise = request.promise();
				(request.close)(cx, &promise, None)?;
			}
			requests.clear();
		}

		Ok(())
	}

	pub(crate) fn error(&mut self, cx: &Context, error: &Value) -> Result<()> {
		if self.state != State::Readable {
			return Err(Error::new("Cannot Error Stream", None));
		}
		self.state = State::Errored;
		self.error = Some(Heap::boxed(error.get()));

		let (requests, closed) = match self.native_reader(cx)? {
			Some(reader) => reader.requests_closed(),
			None => return Ok(()),
		};

		closed.reject(cx, error);
		for request in &*requests {
			let promise = request.promise();
			(request.error)(cx, &promise, error);
		}
		requests.clear();

		Ok(())
	}

	pub(crate) fn stored_error(&self) -> Value {
		self.error.as_ref().map_or_else(Value::undefined_handle, |error| {
			Value::from(unsafe { Local::from_heap(error) })
		})
	}

	pub(crate) fn native_controller(&self, cx: &Context) -> Result<Controller> {
		match self.controller_kind {
			ControllerKind::Default => {
				let controller = Object::from(unsafe { Local::from_heap(&self.controller) });
				let controller = DefaultController::get_mut_private(cx, &controller)?;
				Ok(Controller::Default(controller))
			}
			ControllerKind::ByteStream => {
				let controller = Object::from(unsafe { Local::from_heap(&self.controller) });
				let controller = ByteStreamController::get_mut_private(cx, &controller)?;
				Ok(Controller::ByteStream(controller))
			}
		}
	}

	pub(crate) fn native_reader(&self, cx: &Context) -> Result<Option<Reader>> {
		match self.reader_kind {
			ReaderKind::None => Ok(None),
			ReaderKind::Default => {
				let reader = Object::from(unsafe { Local::from_heap(self.reader.as_ref().unwrap()) });
				let reader = DefaultReader::get_mut_private(cx, &reader)?;
				Ok(Some(Reader::Default(reader)))
			}
			ReaderKind::Byob => {
				let reader = Object::from(unsafe { Local::from_heap(self.reader.as_ref().unwrap()) });
				let reader = ByobReader::get_mut_private(cx, &reader)?;
				Ok(Some(Reader::Byob(reader)))
			}
		}
	}
}
