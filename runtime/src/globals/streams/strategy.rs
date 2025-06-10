/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::any::TypeId;
use std::slice;

use ion::class::Reflector;
use ion::conversions::FromValue;
use ion::flags::PropertyFlags;
use ion::{
	ClassDefinition as _, Context, FromValue, Function, Local, Object, Result, ResultExc, Traceable, Value, js_class,
	js_fn,
};
use mozjs::jsapi::{CallArgs, Heap, JS_GetFunctionObject, JS_GetObjectFunction, JSContext, JSFunction};
use mozjs::jsval::{Int32Value, JSVal, ObjectValue, UndefinedValue};

use crate::ContextExt as _;

#[derive(Debug, Traceable)]
pub enum SizeCallback {
	None,
	Script(Box<Heap<*mut JSFunction>>),
	ByteLength,
	Count,
}

impl SizeCallback {
	pub(crate) fn compute<'cx>(&self, cx: &'cx Context, chunk: &Value) -> ResultExc<Value<'cx>> {
		match self {
			SizeCallback::None | SizeCallback::Count => Ok(Value::i32(cx, 1)),
			SizeCallback::Script(cb) => {
				let cb = Function::from(unsafe { Local::from_heap(cb) });
				cb.call(cx, &Object::null(cx), slice::from_ref(chunk))
					.map_err(|report| report.unwrap().exception)
			}
			SizeCallback::ByteLength => {
				let chunk = Object::from_value(cx, chunk, true, ())?;
				Ok(chunk.get(cx, "byteLength")?.unwrap_or_else(Value::undefined_handle))
			}
		}
	}
}

#[derive(Debug)]
pub enum QueuingStrategy<'cx> {
	Script(ScriptQueuingStrategy<'cx>),
	ByteLength(f64),
	Count(f64),
}

impl QueuingStrategy<'_> {
	pub(crate) fn high_water_mark(&self) -> Option<f64> {
		match self {
			QueuingStrategy::Script(strategy) => strategy.high_water_mark,
			QueuingStrategy::ByteLength(high_water_mark) | QueuingStrategy::Count(high_water_mark) => {
				Some(*high_water_mark)
			}
		}
	}

	pub(crate) fn has_size(&self) -> bool {
		match self {
			QueuingStrategy::Script(strategy) => strategy.size.is_some(),
			QueuingStrategy::ByteLength(_) | QueuingStrategy::Count(_) => true,
		}
	}

	pub(crate) fn size(&self) -> SizeCallback {
		match self {
			QueuingStrategy::Script(strategy) => strategy.size.as_ref().map_or(SizeCallback::None, |strategy| {
				SizeCallback::Script(Heap::boxed(strategy.get()))
			}),
			QueuingStrategy::ByteLength(_) => SizeCallback::ByteLength,
			QueuingStrategy::Count(_) => SizeCallback::Count,
		}
	}
}

impl<'cx> Default for QueuingStrategy<'cx> {
	fn default() -> QueuingStrategy<'cx> {
		QueuingStrategy::Script(ScriptQueuingStrategy::default())
	}
}

impl<'cx> FromValue<'cx> for QueuingStrategy<'cx> {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, strict: bool, _: ()) -> Result<QueuingStrategy<'cx>> {
		let object = Object::from_value(cx, value, strict, ())?;
		if let Ok(byte_length) = ByteLengthQueuingStrategy::get_private(cx, &object) {
			return Ok(QueuingStrategy::ByteLength(byte_length.high_water_mark));
		}

		ScriptQueuingStrategy::from_value(cx, value, strict, ()).map(QueuingStrategy::Script)
	}
}

#[derive(Debug, Default, FromValue)]
pub struct ScriptQueuingStrategy<'cx> {
	high_water_mark: Option<f64>,
	size: Option<Function<'cx>>,
}

#[derive(Debug, FromValue)]
pub struct QueuingStrategyInit {
	high_water_mark: f64,
}

#[js_fn]
fn byte_length_size(cx: &Context, object: Object) -> Result<JSVal> {
	Ok(object.get_as(cx, "byteLength", false, ())?.unwrap_or_else(UndefinedValue))
}

#[js_class]
pub(crate) struct ByteLengthQueuingStrategy {
	reflector: Reflector,
	high_water_mark: f64,
}

#[js_class]
impl ByteLengthQueuingStrategy {
	#[ion(constructor)]
	pub fn constructor(cx: &Context, options: QueuingStrategyInit) -> ByteLengthQueuingStrategy {
		let key = TypeId::of::<ByteLengthQueuingStrategy>();
		unsafe {
			cx.get_private().globals.entry(key).or_insert_with(|| {
				let size = Function::new(cx, "size", Some(byte_length_size), 1, PropertyFlags::empty());
				Heap::boxed(ObjectValue(JS_GetFunctionObject(size.get())))
			});
		}

		ByteLengthQueuingStrategy {
			reflector: Reflector::default(),
			high_water_mark: options.high_water_mark,
		}
	}

	#[ion(get)]
	pub fn get_high_water_mark(&self) -> f64 {
		self.high_water_mark
	}

	#[ion(get)]
	#[expect(clippy::unused_self)]
	pub fn get_size(&self, cx: &Context) -> *mut JSFunction {
		let key = TypeId::of::<ByteLengthQueuingStrategy>();
		unsafe { JS_GetObjectFunction(cx.get_private().globals.get(&key).unwrap().get().to_object()) }
	}
}

unsafe extern "C" fn count_size(_cx: *mut JSContext, argc: u32, vp: *mut JSVal) -> bool {
	unsafe {
		CallArgs::from_vp(vp, argc).rval().set(Int32Value(1));
	}
	true
}

#[js_class]
pub(crate) struct CountQueuingStrategy {
	reflector: Reflector,
	high_water_mark: f64,
}

#[js_class]
impl CountQueuingStrategy {
	#[ion(constructor)]
	pub fn constructor(cx: &Context, options: QueuingStrategyInit) -> CountQueuingStrategy {
		let key = TypeId::of::<CountQueuingStrategy>();
		unsafe {
			cx.get_private().globals.entry(key).or_insert_with(|| {
				let size = Function::new(cx, "size", Some(count_size), 0, PropertyFlags::empty());
				Heap::boxed(ObjectValue(JS_GetFunctionObject(size.get())))
			});
		}

		CountQueuingStrategy {
			reflector: Reflector::default(),
			high_water_mark: options.high_water_mark,
		}
	}

	#[ion(get)]
	pub fn get_high_water_mark(&self) -> f64 {
		self.high_water_mark
	}

	#[ion(get)]
	#[expect(clippy::unused_self)]
	pub fn get_size(&self, cx: &Context) -> *mut JSFunction {
		let key = TypeId::of::<CountQueuingStrategy>();
		unsafe { JS_GetObjectFunction(cx.get_private().globals.get(&key).unwrap().get().to_object()) }
	}
}
