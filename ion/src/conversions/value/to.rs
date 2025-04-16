/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::borrow::Cow;
use std::ptr::NonNull;
use std::rc::Rc;

use mozjs::jsapi::{
	JSFunction, JSObject, JSString, JS_GetFunctionObject, JS_IdToValue, JS_NewStringCopyN, JS_WrapValue,
	PropertyKey as JSPropertyKey, Symbol as JSSymbol,
};
use mozjs::jsval::{
	BooleanValue, DoubleValue, Int32Value, JSVal, NullValue, ObjectOrNullValue, ObjectValue, StringValue, SymbolValue,
	UInt32Value, UndefinedValue,
};
use mozjs::rust::{maybe_wrap_object_or_null_value, maybe_wrap_object_value, maybe_wrap_value};
use mozjs::typedarray as jsta;

use crate::object::RegExp;
use crate::string::byte::{BytePredicate, ByteStr, ByteString};
use crate::typedarray::{ArrayBuffer, TypedArray, TypedArrayElement};
use crate::{Array, Context, Date, Function, Object, Promise, PropertyKey, Symbol, Value};

/// Represents types that can be converted to JavaScript [Values](Value).
pub trait ToValue<'cx> {
	/// Converts `self` to a [`Value`] and stores it in `value`.
	fn to_value(&self, cx: &'cx Context, value: &mut Value);

	/// Converts `self` to a new [`Value`].
	fn as_value(&self, cx: &'cx Context) -> Value<'cx> {
		let mut value = Value::undefined(cx);
		self.to_value(cx, &mut value);
		value
	}
}

impl ToValue<'_> for () {
	fn to_value(&self, _: &Context, value: &mut Value) {
		value.handle_mut().set(UndefinedValue());
	}
}

impl ToValue<'_> for bool {
	fn to_value(&self, _: &Context, value: &mut Value) {
		value.handle_mut().set(BooleanValue(*self));
	}
}

macro_rules! impl_to_value_for_integer {
	($ty:ty, signed) => {
		impl ToValue<'_> for $ty {
			fn to_value(&self, _: &Context, value: &mut Value) {
				value.handle_mut().set(Int32Value(i32::from(*self)));
			}
		}
	};
	($ty:ty, unsigned) => {
		impl ToValue<'_> for $ty {
			fn to_value(&self, _: &Context, value: &mut Value) {
				value.handle_mut().set(UInt32Value(u32::from(*self)));
			}
		}
	};
}

impl_to_value_for_integer!(i8, signed);
impl_to_value_for_integer!(i16, signed);
impl_to_value_for_integer!(i32, signed);

impl_to_value_for_integer!(u8, unsigned);
impl_to_value_for_integer!(u16, unsigned);
impl_to_value_for_integer!(u32, unsigned);

macro_rules! impl_to_value_as_double {
	($ty:ty, int) => {
		impl ToValue<'_> for $ty {
			fn to_value(&self, _: &Context, value: &mut Value) {
				value.handle_mut().set(DoubleValue(*self as f64));
			}
		}
	};
	($ty:ty, float) => {
		impl ToValue<'_> for $ty {
			fn to_value(&self, _: &Context, value: &mut Value) {
				value.handle_mut().set(DoubleValue(f64::from(*self)));
			}
		}
	};
}

impl_to_value_as_double!(i64, int);
impl_to_value_as_double!(u64, int);
impl_to_value_as_double!(f32, float);
impl_to_value_as_double!(f64, float);

impl ToValue<'_> for *mut JSString {
	fn to_value(&self, cx: &Context, value: &mut Value) {
		value.handle_mut().set(StringValue(unsafe { &**self }));
		unsafe {
			JS_WrapValue(cx.as_ptr(), value.handle_mut().into());
		}
	}
}

impl<'cx> ToValue<'cx> for crate::String<'cx> {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		self.handle().to_value(cx, value);
	}
}

impl ToValue<'_> for str {
	fn to_value(&self, cx: &Context, value: &mut Value) {
		let string = crate::String::copy_from_str(cx, self);
		if let Some(string) = string {
			string.to_value(cx, value);
		} else {
			panic!("Failed to Instantiate String");
		}
	}
}

impl<'cx> ToValue<'cx> for String {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		(**self).to_value(cx, value);
	}
}

impl<'cx, T: ToOwned + ToValue<'cx>> ToValue<'cx> for Cow<'_, T> {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		self.as_ref().to_value(cx, value)
	}
}

impl<'cx, T: BytePredicate> ToValue<'cx> for ByteStr<T> {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		unsafe { JS_NewStringCopyN(cx.as_ptr(), self.as_ptr().cast(), self.len()).to_value(cx, value) }
	}
}

impl<'cx, T: BytePredicate> ToValue<'cx> for ByteString<T> {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		(**self).to_value(cx, value)
	}
}

impl ToValue<'_> for *mut JSObject {
	fn to_value(&self, cx: &Context, value: &mut Value) {
		value.handle_mut().set(ObjectOrNullValue(*self));
		unsafe {
			maybe_wrap_object_or_null_value(cx.as_ptr(), value.handle_mut());
		}
	}
}

impl ToValue<'_> for NonNull<JSObject> {
	fn to_value(&self, cx: &Context, value: &mut Value) {
		value.handle_mut().set(ObjectValue(self.as_ptr()));
		unsafe {
			maybe_wrap_object_value(cx.as_ptr(), value.handle_mut());
		}
	}
}

impl<'cx> ToValue<'cx> for Object<'cx> {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		self.handle().to_value(cx, value);
	}
}

impl<'cx> ToValue<'cx> for Array<'cx> {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		self.handle().to_value(cx, value);
	}
}

impl<'cx> ToValue<'cx> for Date<'cx> {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		self.handle().to_value(cx, value);
	}
}

impl<'cx> ToValue<'cx> for Promise<'cx> {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		self.handle().to_value(cx, value);
	}
}

impl<'cx> ToValue<'cx> for RegExp<'cx> {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		self.handle().to_value(cx, value);
	}
}

impl<T: jsta::TypedArrayElement, S: jsta::JSObjectStorage> ToValue<'_> for jsta::TypedArray<T, S> {
	fn to_value(&self, cx: &Context, value: &mut Value) {
		unsafe { self.underlying_object().as_raw().to_value(cx, value) }
	}
}

impl<'cx> ToValue<'cx> for ArrayBuffer<'cx> {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		self.handle().to_value(cx, value)
	}
}

impl<'cx, T: TypedArrayElement> ToValue<'cx> for TypedArray<'cx, T> {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		self.handle().to_value(cx, value)
	}
}

impl<'cx> ToValue<'cx> for *mut JSFunction {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		unsafe { JS_GetFunctionObject(*self) }.to_value(cx, value);
	}
}

impl<'cx> ToValue<'cx> for Function<'cx> {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		self.handle().to_value(cx, value);
	}
}

impl ToValue<'_> for *mut JSSymbol {
	fn to_value(&self, _: &Context, value: &mut Value) {
		value.handle_mut().set(SymbolValue(unsafe { &**self }));
	}
}

impl<'cx> ToValue<'cx> for Symbol<'cx> {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		self.handle().to_value(cx, value);
	}
}

impl ToValue<'_> for JSVal {
	fn to_value(&self, cx: &Context, value: &mut Value) {
		value.handle_mut().set(*self);
		unsafe {
			maybe_wrap_value(cx.as_ptr(), value.handle_mut());
		}
	}
}

impl<'cx> ToValue<'cx> for Value<'cx> {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		self.handle().to_value(cx, value);
	}
}

impl<'cx> ToValue<'cx> for JSPropertyKey {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		unsafe {
			JS_IdToValue(cx.as_ptr(), *self, value.handle_mut().into());
		}
	}
}

impl<'cx> ToValue<'cx> for PropertyKey<'cx> {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		self.handle().to_value(cx, value);
	}
}

impl<'cx, T: ToValue<'cx> + ?Sized> ToValue<'cx> for &'_ T {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		(*self).to_value(cx, value);
	}
}

impl<'cx, T: ToValue<'cx> + ?Sized> ToValue<'cx> for Box<T> {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		(**self).to_value(cx, value);
	}
}

impl<'cx, T: ToValue<'cx> + ?Sized> ToValue<'cx> for Rc<T> {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		(**self).to_value(cx, value);
	}
}

impl<'cx, T: ToValue<'cx>> ToValue<'cx> for Option<T> {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		match self {
			Some(t) => t.to_value(cx, value),
			None => value.handle_mut().set(NullValue()),
		};
	}
}

impl<'cx, T: ToValue<'cx>> ToValue<'cx> for [T] {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		let array = Array::new_with_length(cx, self.len());

		for (i, t) in self.iter().enumerate() {
			assert!(array.set_as(cx, i as u32, t));
		}

		array.to_value(cx, value);
	}
}

impl<'cx, const N: usize, T: ToValue<'cx>> ToValue<'cx> for [T; N] {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		<[T]>::to_value(self, cx, value);
	}
}

impl<'cx, T: ToValue<'cx>> ToValue<'cx> for Vec<T> {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		(**self).to_value(cx, value);
	}
}
