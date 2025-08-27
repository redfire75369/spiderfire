/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::ffi::{CStr, CString};
use std::ops::Deref;
use std::ptr::NonNull;

use mozjs::conversions::jsstr_to_string;
use mozjs::gc::{RootableVec, RootedVec};
use mozjs::jsapi::{
	HandleValueArray, JS_CallFunction, JS_DecompileFunction, JS_GetFunctionArity, JS_GetFunctionDisplayId,
	JS_GetFunctionId, JS_GetFunctionLength, JS_GetFunctionObject, JS_GetObjectFunction, JS_IsBuiltinEvalFunction,
	JS_IsBuiltinFunctionConstructor, JS_IsConstructor, JS_NewFunction, JS_ObjectIsFunction, JSContext, JSFunction,
	JSFunctionSpec, JSObject, NewFunctionFromSpec1, NewFunctionWithReserved, SetFunctionNativeReserved,
};
use mozjs::jsval::{JSVal, ObjectValue};

use crate::flags::PropertyFlags;
use crate::function::closure::{
	Closure, ClosureOnce, call_closure, call_closure_once, create_closure_object, create_closure_once_object,
};
use crate::{Context, Error, ErrorReport, Local, Object, Value};

/// Native Function that can be used from JavaScript.
pub type NativeFunction = unsafe extern "C" fn(*mut JSContext, u32, *mut JSVal) -> bool;

/// Represents a [Function] within the JavaScript Runtime.
/// Refer to [MDN](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Functions) for more details.
#[derive(Debug)]
pub struct Function<'f> {
	function: Local<'f, *mut JSFunction>,
}

impl<'f> Function<'f> {
	/// Creates a new [Function] with the given name, native function, number of arguments and flags.
	pub fn new(
		cx: &'f Context, name: &str, func: Option<NativeFunction>, nargs: u32, flags: PropertyFlags,
	) -> Function<'f> {
		let name = CString::new(name).unwrap();
		Function {
			function: cx
				.root(unsafe { JS_NewFunction(cx.as_ptr(), func, nargs, u32::from(flags.bits()), name.as_ptr()) }),
		}
	}

	/// Creates a new [Function] with the given [`spec`](JSFunctionSpec).
	pub fn from_spec(cx: &'f Context, spec: &JSFunctionSpec) -> Function<'f> {
		Function {
			function: cx.root(unsafe { NewFunctionFromSpec1(cx.as_ptr(), spec) }),
		}
	}

	/// Creates a new [Function] with a [ClosureOnce].
	///
	/// Throws a JS Exception if called more than once.
	pub fn from_closure_once(
		cx: &'f Context, name: &CStr, closure: Box<ClosureOnce>, nargs: u32, flags: PropertyFlags,
	) -> Function<'f> {
		let closure = create_closure_once_object(cx, closure);
		Function::create_with_closure(cx, call_closure_once, name, &closure, nargs, flags)
	}

	/// Creates a new [Function] with a [Closure].
	pub fn from_closure(
		cx: &'f Context, name: &CStr, closure: Box<Closure>, nargs: u32, flags: PropertyFlags,
	) -> Function<'f> {
		let closure = create_closure_object(cx, closure);
		Function::create_with_closure(cx, call_closure, name, &closure, nargs, flags)
	}

	fn create_with_closure(
		cx: &'f Context, call: NativeFunction, name: &CStr, closure: &Object, nargs: u32, flags: PropertyFlags,
	) -> Function<'f> {
		unsafe {
			let function = Function {
				function: cx.root(NewFunctionWithReserved(
					cx.as_ptr(),
					Some(call),
					nargs,
					u32::from(flags.bits()),
					name.as_ptr().cast(),
				)),
			};
			SetFunctionNativeReserved(
				JS_GetFunctionObject(function.get()),
				0,
				&ObjectValue(closure.handle().get()),
			);
			function
		}
	}

	/// Creates a new [Function] from an object.
	/// Returns [None] if the object is not a function.
	pub fn from_object(cx: &'f Context, obj: &Local<'_, *mut JSObject>) -> Option<Function<'f>> {
		(unsafe { Function::is_function_raw(obj.get()) }).then(|| Function {
			function: cx.root(unsafe { JS_GetObjectFunction(obj.get()) }),
		})
	}

	/// Converts the [Function] into an [Object].
	pub fn to_object(&self, cx: &'f Context) -> Object<'f> {
		cx.root(unsafe { JS_GetFunctionObject(self.get()) }).into()
	}

	/// Converts the [Function] into a [String] in the form of its definition/source.
	pub fn to_string(&self, cx: &Context) -> Option<String> {
		unsafe {
			NonNull::new(JS_DecompileFunction(cx.as_ptr(), self.handle().into()))
				.map(|str| jsstr_to_string(cx.as_ptr(), str))
		}
	}

	/// Returns the name of the function.
	pub fn name(&self, cx: &Context) -> crate::Result<String> {
		let mut name = crate::String::new(cx);
		if unsafe { JS_GetFunctionId(cx.as_ptr(), self.handle().into(), name.handle_mut().into()) } {
			name.to_owned(cx)
		} else {
			Err(Error::none())
		}
	}

	/// Returns the display name of the function.
	/// Function display names are a non-standard feature.
	/// Refer to [MDN](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Function/displayName) for more details.
	pub fn display_name(&self, cx: &Context) -> crate::Result<String> {
		let mut name = crate::String::new(cx);
		if unsafe { JS_GetFunctionDisplayId(cx.as_ptr(), self.handle().into(), name.handle_mut().into()) } {
			name.to_owned(cx)
		} else {
			Err(Error::none())
		}
	}

	/// Returns the number of arguments of the function.
	pub fn nargs(&self) -> u16 {
		unsafe { JS_GetFunctionArity(self.get()) }
	}

	/// Returns the length of the source of the function.
	pub fn length(&self, cx: &Context) -> Option<u16> {
		let mut length = 0;
		unsafe { JS_GetFunctionLength(cx.as_ptr(), self.handle().into(), &raw mut length) }.then_some(length)
	}

	/// Calls the [Function] with the given `this` [Object] and arguments.
	/// Returns the result of the [Function] as a [Value].
	/// Returns [Err] if the function call fails or an exception occurs.
	pub fn call<'cx>(
		&self, cx: &'cx Context, this: &Object, args: &[Value],
	) -> Result<Value<'cx>, Option<ErrorReport>> {
		let mut root = RootableVec::new_unrooted();
		let args = RootedVec::from_iter(&mut root, args.iter().map(|a| a.get()));
		self.call_with_handle(cx, this, HandleValueArray::from(&args))
	}

	/// Calls the [Function] with the given `this` [Object] and arguments as a [HandleValueArray].
	/// Returns the result of the [Function] as a [Value].
	/// Returns [Err] if the function call fails or an exception occurs.
	pub fn call_with_handle<'cx>(
		&self, cx: &'cx Context, this: &Object, args: HandleValueArray,
	) -> Result<Value<'cx>, Option<ErrorReport>> {
		let mut rval = Value::undefined(cx);
		if unsafe {
			JS_CallFunction(
				cx.as_ptr(),
				this.handle().into(),
				self.handle().into(),
				&raw const args,
				rval.handle_mut().into(),
			)
		} {
			Ok(rval)
		} else {
			Err(ErrorReport::new_with_exception_stack(cx).unwrap())
		}
	}

	/// Checks if the [Function] is the built-in eval function.
	pub fn is_eval(&self) -> bool {
		unsafe { JS_IsBuiltinEvalFunction(self.get()) }
	}

	/// Checks if the [Function] is a constructor.
	pub fn is_constructor(&self) -> bool {
		unsafe { JS_IsConstructor(self.get()) }
	}

	/// Checks if the [Function] is the built-in function constructor.
	pub fn is_function_constructor(&self) -> bool {
		unsafe { JS_IsBuiltinFunctionConstructor(self.get()) }
	}

	/// Checks if [a raw object](*mut JSObject) is a function.
	pub unsafe fn is_function_raw(obj: *mut JSObject) -> bool {
		unsafe { JS_ObjectIsFunction(obj) }
	}
}

impl<'f> From<Local<'f, *mut JSFunction>> for Function<'f> {
	fn from(function: Local<'f, *mut JSFunction>) -> Function<'f> {
		Function { function }
	}
}

impl<'f> Deref for Function<'f> {
	type Target = Local<'f, *mut JSFunction>;

	fn deref(&self) -> &Self::Target {
		&self.function
	}
}
