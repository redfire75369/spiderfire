/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

use mozjs::glue::JS_GetReservedSlot;
use mozjs::jsapi::{
	GCContext, GetFunctionNativeReserved, JS_NewObject, JS_SetReservedSlot, JSCLASS_BACKGROUND_FINALIZE, JSClass,
	JSClassOps, JSContext, JSObject,
};
use mozjs::jsval::{JSVal, PrivateValue, UndefinedValue};

use crate::conversions::ToValue as _;
use crate::function::__handle_native_function_result;
use crate::object::class_reserved_slots;
use crate::{Arguments, Context, Error, ErrorKind, Object, ResultExc, ThrowException as _, Value};

const CLOSURE_SLOT: u32 = 0;

pub type ClosureOnce = dyn for<'cx> FnOnce(&mut Arguments<'cx>) -> ResultExc<Value<'cx>> + 'static;
pub type Closure = dyn for<'cx> FnMut(&mut Arguments<'cx>) -> ResultExc<Value<'cx>> + 'static;

type ClosurePrivate = Box<Closure>;
type ClosureOncePrivate = Option<Box<ClosureOnce>>;

pub(crate) fn create_closure_once_object(cx: &Context, closure: Box<ClosureOnce>) -> Object<'_> {
	unsafe {
		let object = Object::from(cx.root(JS_NewObject(cx.as_ptr(), &raw const CLOSURE_ONCE_CLASS)));
		JS_SetReservedSlot(
			object.handle().get(),
			CLOSURE_SLOT,
			&PrivateValue(Box::into_raw(Box::new(Some(closure))).cast_const().cast()),
		);
		object
	}
}

pub(crate) fn create_closure_object(cx: &Context, closure: Box<Closure>) -> Object<'_> {
	unsafe {
		let object = Object::from(cx.root(JS_NewObject(cx.as_ptr(), &raw const CLOSURE_CLASS)));
		JS_SetReservedSlot(
			object.handle().get(),
			CLOSURE_SLOT,
			&PrivateValue(Box::into_raw(Box::new(closure)).cast_const().cast()),
		);
		object
	}
}

fn get_function_reserved<'cx>(cx: &'cx Context, args: &Arguments) -> Value<'cx> {
	let callee = args.callee();
	Value::from(cx.root(unsafe { *GetFunctionNativeReserved(callee.handle().get(), 0) }))
}

fn get_reserved(object: *mut JSObject, slot: u32) -> JSVal {
	let mut value = UndefinedValue();
	unsafe { JS_GetReservedSlot(object, slot, &raw mut value) };
	value
}

pub(crate) unsafe extern "C" fn call_closure_once(cx: *mut JSContext, argc: u32, vp: *mut JSVal) -> bool {
	let cx = &unsafe { Context::new_unchecked(cx) };
	let args = &mut unsafe { Arguments::new(cx, argc, vp) };

	let reserved = get_function_reserved(cx, args);
	let value = get_reserved(reserved.handle().to_object(), CLOSURE_SLOT);
	let closure = unsafe { &mut *(value.to_private().cast::<ClosureOncePrivate>().cast_mut()) };

	if let Some(closure) = closure.take() {
		let result = catch_unwind(AssertUnwindSafe(|| {
			closure(args).map(|result| result.to_value(cx, &mut args.rval()))
		}));
		__handle_native_function_result(cx, result)
	} else {
		Error::new("ClosureOnce was called more than once.", ErrorKind::Type).throw(cx);
		false
	}
}

pub(crate) unsafe extern "C" fn call_closure(cx: *mut JSContext, argc: u32, vp: *mut JSVal) -> bool {
	let cx = &unsafe { Context::new_unchecked(cx) };
	let args = &mut unsafe { Arguments::new(cx, argc, vp) };

	let reserved = get_function_reserved(cx, args);
	let value = get_reserved(reserved.handle().to_object(), CLOSURE_SLOT);
	let closure = unsafe { &mut *(value.to_private().cast::<ClosurePrivate>().cast_mut()) };

	let result = catch_unwind(AssertUnwindSafe(|| {
		closure(args).map(|result| result.to_value(cx, &mut args.rval()))
	}));
	__handle_native_function_result(cx, result)
}

unsafe extern "C" fn finalise_closure<T>(_: *mut GCContext, object: *mut JSObject) {
	let mut value = UndefinedValue();
	unsafe {
		JS_GetReservedSlot(object, CLOSURE_SLOT, &raw mut value);
		let _ = Box::from_raw(value.to_private().cast::<T>().cast_mut());
	}
}

static CLOSURE_ONCE_OPS: JSClassOps = JSClassOps {
	addProperty: None,
	delProperty: None,
	enumerate: None,
	newEnumerate: None,
	resolve: None,
	mayResolve: None,
	finalize: Some(finalise_closure::<ClosureOncePrivate>),
	call: None,
	construct: None,
	trace: None,
};

static CLOSURE_ONCE_CLASS: JSClass = JSClass {
	name: c"ClosureOnce".as_ptr(),
	flags: JSCLASS_BACKGROUND_FINALIZE | class_reserved_slots(1),
	cOps: &raw const CLOSURE_ONCE_OPS,
	spec: ptr::null_mut(),
	ext: ptr::null_mut(),
	oOps: ptr::null_mut(),
};

static CLOSURE_OPS: JSClassOps = JSClassOps {
	addProperty: None,
	delProperty: None,
	enumerate: None,
	newEnumerate: None,
	resolve: None,
	mayResolve: None,
	finalize: Some(finalise_closure::<ClosurePrivate>),
	call: None,
	construct: None,
	trace: None,
};

static CLOSURE_CLASS: JSClass = JSClass {
	name: c"Closure".as_ptr(),
	flags: JSCLASS_BACKGROUND_FINALIZE | class_reserved_slots(1),
	cOps: &raw const CLOSURE_OPS,
	spec: ptr::null_mut(),
	ext: ptr::null_mut(),
	oOps: ptr::null_mut(),
};
