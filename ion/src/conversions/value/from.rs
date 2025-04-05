/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */
use std::ptr;

pub use mozjs::conversions::ConversionBehavior;
use mozjs::conversions::{ConversionResult, FromJSValConvertible};
use mozjs::jsapi::{
	AssertSameCompartment, AssertSameCompartment1, ForOfIterator, ForOfIterator_NonIterableBehavior, JSFunction,
	JSObject, JSString, RootedObject, RootedValue, Symbol as JSSymbol,
};
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rust::{ToBoolean, ToNumber, ToString};
use mozjs::typedarray as jsta;
use mozjs::typedarray::JSObjectStorage;

use crate::object::RegExp;
use crate::string::byte::{BytePredicate, ByteString};
use crate::typedarray::{ArrayBuffer, TypedArray, TypedArrayElement};
use crate::{
	Array, Context, Date, Error, ErrorKind, Exception, Function, Object, Promise, Result, StringRef, Symbol, Value,
};

/// Represents types that can be converted to from [JavaScript Values](Value).
pub trait FromValue<'cx>: Sized {
	type Config;

	/// Converts `value` to the desired type.
	/// `strict` and `config` determine the strictness of the conversion and specify additional conversion constraints respectively.
	/// Returns [Err] with the [error](Error) if conversion fails.
	fn from_value(cx: &'cx Context, value: &Value, strict: bool, config: Self::Config) -> Result<Self>;
}

impl<'cx> FromValue<'cx> for bool {
	type Config = ();

	fn from_value(_: &'cx Context, value: &Value, strict: bool, _: ()) -> Result<bool> {
		let value = value.handle();
		if value.is_boolean() {
			return Ok(value.to_boolean());
		}

		if strict {
			Err(Error::new("Expected Boolean in Strict Conversion", ErrorKind::Type))
		} else {
			Ok(unsafe { ToBoolean(value) })
		}
	}
}

macro_rules! impl_from_value_for_integer {
	($ty:ty) => {
		impl<'cx> FromValue<'cx> for $ty {
			type Config = ConversionBehavior;

			fn from_value(cx: &'cx Context, value: &Value, strict: bool, config: ConversionBehavior) -> Result<$ty> {
				let value = value.handle();
				if strict && !value.is_number() {
					return Err(Error::new(
						"Expected Number in Strict Conversion",
						ErrorKind::Type,
					));
				}

				match unsafe { <$ty>::from_jsval(cx.as_ptr(), value, config) } {
					Ok(ConversionResult::Success(number)) => Ok(number),
					Err(_) => Err(Exception::new(cx)?.unwrap().to_error()),
					_ => unreachable!(),
				}
			}
		}
	};
}

impl_from_value_for_integer!(u8);
impl_from_value_for_integer!(u16);
impl_from_value_for_integer!(u32);
impl_from_value_for_integer!(u64);

impl_from_value_for_integer!(i8);
impl_from_value_for_integer!(i16);
impl_from_value_for_integer!(i32);
impl_from_value_for_integer!(i64);

impl<'cx> FromValue<'cx> for f32 {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, strict: bool, _: ()) -> Result<f32> {
		f64::from_value(cx, value, strict, ()).map(|float| float as f32)
	}
}

impl<'cx> FromValue<'cx> for f64 {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, strict: bool, _: ()) -> Result<f64> {
		let value = value.handle();
		if strict && !value.is_number() {
			return Err(Error::new("Expected Number in Strict Conversion", ErrorKind::Type));
		}

		let number = unsafe { ToNumber(cx.as_ptr(), value) };
		number.map_err(|_| Error::new("Unable to Convert Value to Number", ErrorKind::Type))
	}
}

impl<'cx> FromValue<'cx> for *mut JSString {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, strict: bool, _: ()) -> Result<*mut JSString> {
		let value = value.handle();
		if strict && !value.is_string() {
			return Err(Error::new("Expected String in Strict Conversion", ErrorKind::Type));
		}

		let str = unsafe { ToString(cx.as_ptr(), value) };
		if str.is_null() {
			Err(Error::new("Failed to convert value to String", ErrorKind::Type))
		} else {
			Ok(str)
		}
	}
}

impl<'cx> FromValue<'cx> for crate::String<'cx> {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, strict: bool, config: ()) -> Result<crate::String<'cx>> {
		<*mut JSString>::from_value(cx, value, strict, config).map(|str| crate::String::from(cx.root(str)))
	}
}

impl<'cx> FromValue<'cx> for StringRef<'cx> {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, strict: bool, config: ()) -> Result<StringRef<'cx>> {
		crate::String::from_value(cx, value, strict, config).map(|str| str.as_ref(cx))
	}
}

impl<'cx, T: BytePredicate> FromValue<'cx> for ByteString<T> {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, strict: bool, config: ()) -> Result<ByteString<T>> {
		const INVALID_CHARACTERS: &str = "ByteString contains invalid characters";
		let string = StringRef::from_value(cx, value, strict, config)?;
		match string {
			StringRef::Latin1(bstr) => {
				ByteString::from(bstr.to_vec()).ok_or_else(|| Error::new(INVALID_CHARACTERS, ErrorKind::Type))
			}
			StringRef::Utf16(wstr) => {
				let bytes = wstr
					.as_bytes()
					.chunks_exact(2)
					.map(|chunk| {
						let codepoint = u16::from_ne_bytes([chunk[0], chunk[1]]);
						u8::try_from(codepoint).map_err(|_| Error::new(INVALID_CHARACTERS, ErrorKind::Type))
					})
					.collect::<Result<Vec<_>>>()?;
				ByteString::from(bytes).ok_or_else(|| Error::new(INVALID_CHARACTERS, ErrorKind::Type))
			}
		}
	}
}

impl<'cx> FromValue<'cx> for String {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, strict: bool, config: ()) -> Result<String> {
		crate::String::from_value(cx, value, strict, config).and_then(|s| s.to_owned(cx))
	}
}

impl<'cx> FromValue<'cx> for *mut JSObject {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, _: bool, _: ()) -> Result<*mut JSObject> {
		let value = value.handle();
		if !value.is_object() {
			return Err(Error::new("Expected Object", ErrorKind::Type));
		}
		let object = value.to_object();
		unsafe {
			AssertSameCompartment(cx.as_ptr(), object);
		}

		Ok(object)
	}
}

impl<'cx> FromValue<'cx> for Object<'cx> {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, _: bool, _: ()) -> Result<Object<'cx>> {
		if !value.handle().is_object() {
			return Err(Error::new("Expected Object", ErrorKind::Type));
		}
		let object = value.to_object(cx);
		unsafe {
			AssertSameCompartment(cx.as_ptr(), object.handle().get());
		}

		Ok(object)
	}
}

impl<'cx> FromValue<'cx> for Array<'cx> {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, _: bool, _: ()) -> Result<Array<'cx>> {
		if !value.handle().is_object() {
			return Err(Error::new("Expected Array", ErrorKind::Type));
		}

		let object = value.to_object(cx).into_local();
		if let Some(array) = Array::from(cx, object) {
			unsafe {
				AssertSameCompartment(cx.as_ptr(), array.handle().get());
			}
			Ok(array)
		} else {
			Err(Error::new("Expected Array", ErrorKind::Type))
		}
	}
}

impl<'cx> FromValue<'cx> for Date<'cx> {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, _: bool, _: ()) -> Result<Date<'cx>> {
		if !value.handle().is_object() {
			return Err(Error::new("Expected Date", ErrorKind::Type));
		}

		let object = value.to_object(cx).into_local();
		if let Some(date) = Date::from(cx, object) {
			unsafe {
				AssertSameCompartment(cx.as_ptr(), date.get());
			}
			Ok(date)
		} else {
			Err(Error::new("Expected Date", ErrorKind::Type))
		}
	}
}

impl<'cx> FromValue<'cx> for Promise<'cx> {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, _: bool, _: ()) -> Result<Promise<'cx>> {
		if !value.handle().is_object() {
			return Err(Error::new("Expected Promise", ErrorKind::Type));
		}

		let object = value.to_object(cx).into_local();
		if let Some(promise) = Promise::from(object) {
			unsafe {
				AssertSameCompartment(cx.as_ptr(), promise.get());
			}
			Ok(promise)
		} else {
			Err(Error::new("Expected Promise", ErrorKind::Type))
		}
	}
}

impl<'cx> FromValue<'cx> for RegExp<'cx> {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, _: bool, _: ()) -> Result<RegExp<'cx>> {
		if !value.handle().is_object() {
			return Err(Error::new("Expected RegExp", ErrorKind::Type));
		}

		let object = value.to_object(cx).into_local();
		if let Some(regexp) = RegExp::from(cx, object) {
			unsafe {
				AssertSameCompartment(cx.as_ptr(), regexp.get());
			}
			Ok(regexp)
		} else {
			Err(Error::new("Expected RegExp", ErrorKind::Type))
		}
	}
}

impl<'cx, T: jsta::TypedArrayElement, S: JSObjectStorage> FromValue<'cx> for jsta::TypedArray<T, S> {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, _: bool, _: ()) -> Result<jsta::TypedArray<T, S>> {
		let value = value.handle();
		if value.is_object() {
			let object = value.to_object();
			cx.root(object);
			jsta::TypedArray::from(object).map_err(|_| Error::new("Expected Typed Array", ErrorKind::Type))
		} else {
			Err(Error::new("Expected Object", ErrorKind::Type))
		}
	}
}

impl<'cx> FromValue<'cx> for ArrayBuffer<'cx> {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, _: bool, _: ()) -> Result<ArrayBuffer<'cx>> {
		if !value.handle().is_object() {
			return Err(Error::new("Expected ArrayBuffer", ErrorKind::Type));
		}

		let object = value.to_object(cx).into_local();
		if let Some(buffer) = ArrayBuffer::from(object) {
			unsafe {
				AssertSameCompartment(cx.as_ptr(), buffer.get());
			}
			Ok(buffer)
		} else {
			Err(Error::new("Expected ArrayBuffer", ErrorKind::Type))
		}
	}
}

impl<'cx, T: TypedArrayElement> FromValue<'cx> for TypedArray<'cx, T> {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, _: bool, _: ()) -> Result<TypedArray<'cx, T>> {
		if !value.handle().is_object() {
			return Err(Error::new("Expected ArrayBuffer", ErrorKind::Type));
		}

		let object = value.to_object(cx).into_local();
		if let Some(array) = TypedArray::from(object) {
			unsafe {
				AssertSameCompartment(cx.as_ptr(), array.get());
			}
			Ok(array)
		} else {
			Err(Error::new("Expected ArrayBuffer", ErrorKind::Type))
		}
	}
}

impl<'cx> FromValue<'cx> for *mut JSFunction {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, strict: bool, config: ()) -> Result<*mut JSFunction> {
		Function::from_value(cx, value, strict, config).map(|f| f.get())
	}
}

impl<'cx> FromValue<'cx> for Function<'cx> {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, _: bool, _: ()) -> Result<Function<'cx>> {
		if !value.handle().is_object() {
			return Err(Error::new("Expected Function", ErrorKind::Type));
		}

		let function_obj = value.to_object(cx);
		if let Some(function) = Function::from_object(cx, &function_obj) {
			unsafe {
				AssertSameCompartment(cx.as_ptr(), function_obj.handle().get());
			}
			Ok(function)
		} else {
			Err(Error::new("Expected Function", ErrorKind::Type))
		}
	}
}

impl<'cx> FromValue<'cx> for *mut JSSymbol {
	type Config = ();

	fn from_value(_: &'cx Context, value: &Value, _: bool, _: ()) -> Result<*mut JSSymbol> {
		let value = value.handle();
		if value.is_symbol() {
			Ok(value.to_symbol())
		} else {
			Err(Error::new("Expected Symbol", ErrorKind::Type))
		}
	}
}

impl<'cx> FromValue<'cx> for Symbol<'cx> {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, strict: bool, config: Self::Config) -> Result<Symbol<'cx>> {
		<*mut JSSymbol>::from_value(cx, value, strict, config).map(|s| cx.root(s).into())
	}
}

impl<'cx> FromValue<'cx> for JSVal {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, _: bool, _: ()) -> Result<JSVal> {
		let value = value.handle();
		unsafe {
			AssertSameCompartment1(cx.as_ptr(), value.into());
		}
		Ok(value.get())
	}
}

impl<'cx> FromValue<'cx> for Value<'cx> {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, _: bool, _: ()) -> Result<Value<'cx>> {
		let value = value.handle();
		unsafe {
			AssertSameCompartment1(cx.as_ptr(), value.into());
		}
		Ok(cx.root(value.get()).into())
	}
}

impl<'cx, T: FromValue<'cx>> FromValue<'cx> for Option<T> {
	type Config = T::Config;

	fn from_value(cx: &'cx Context, value: &Value, strict: bool, config: T::Config) -> Result<Option<T>> {
		if value.handle().is_null_or_undefined() {
			Ok(None)
		} else {
			Ok(Some(T::from_value(cx, value, strict, config)?))
		}
	}
}

// Copied from [rust-mozjs](https://github.com/servo/rust-mozjs/blob/master/src/conversions.rs#L619-L642)
struct ForOfIteratorGuard<'a> {
	root: &'a mut ForOfIterator,
}

impl<'a> ForOfIteratorGuard<'a> {
	fn new(cx: &Context, root: &'a mut ForOfIterator) -> Self {
		cx.root(root.iterator.data);
		ForOfIteratorGuard { root }
	}
}

impl<'cx, T: FromValue<'cx>> FromValue<'cx> for Vec<T>
where
	T::Config: Clone,
{
	type Config = T::Config;

	// Adapted from [rust-mozjs](https://github.com/servo/rust-mozjs/blob/master/src/conversions.rs#L644-L707)
	fn from_value(cx: &'cx Context, value: &Value, strict: bool, config: T::Config) -> Result<Vec<T>> {
		if !value.handle().is_object() {
			return Err(Error::new("Expected Object", ErrorKind::Type));
		}
		let object = value.to_object(cx);
		if strict && !Array::is_array(cx, &object) {
			return Err(Error::new("Expected Array", ErrorKind::Type));
		}

		let mut iterator = ForOfIterator {
			cx_: cx.as_ptr(),
			iterator: RootedObject::new_unrooted(ptr::null_mut()),
			nextMethod: RootedValue::new_unrooted(UndefinedValue()),
			index: u32::MAX, // NOT_ARRAY
		};
		let iterator = ForOfIteratorGuard::new(cx, &mut iterator);
		let iterator = &mut *iterator.root;

		let init = unsafe {
			iterator.init(
				value.handle().into(),
				ForOfIterator_NonIterableBehavior::AllowNonIterable,
			)
		};
		if !init {
			return Err(Error::new("Failed to Initialise Iterator", ErrorKind::Type));
		}

		if iterator.iterator.data.is_null() {
			return Err(Error::new("Expected Iterable", ErrorKind::Type));
		}

		let mut ret = vec![];

		let mut value = Value::undefined(cx);
		loop {
			let mut done = false;
			if unsafe { !iterator.next(value.handle_mut().into(), &mut done) } {
				return Err(Error::new("Failed to Execute Next on Iterator", ErrorKind::Type));
			}

			if done {
				break;
			}
			ret.push(T::from_value(cx, &value, strict, config.clone())?);
		}
		Ok(ret)
	}
}

#[cfg(test)]
mod tests {
	use std::f64::consts::PI;

	use chrono::{TimeZone, Utc};
	use mozjs::conversions::ConversionBehavior;
	use mozjs::gc::{RootableVec, RootedVec};
	use mozjs::jsval::Int32Value;

	use crate::conversions::{FromValue, ToValue};
	use crate::utils::test::TestRuntime;
	use crate::{Array, Date, Object, Promise, Value};

	#[test]
	fn boolean() {
		let rt = TestRuntime::new();
		let cx = &rt.cx;

		let value = Value::bool(cx, false);
		let result = bool::from_value(cx, &value, true, ());
		assert!(!result.unwrap());

		let value = Value::i32(cx, 0);
		let result = bool::from_value(cx, &value, true, ());
		assert!(result.is_err());
		let result = bool::from_value(cx, &value, false, ());
		assert!(!result.unwrap());

		let value = Value::f64(cx, PI);
		let result = bool::from_value(cx, &value, true, ());
		assert!(result.is_err());
		let result = bool::from_value(cx, &value, false, ());
		assert!(result.unwrap());

		let value = Value::string(cx, "");
		let result = bool::from_value(cx, &value, true, ());
		assert!(result.is_err());
		let result = bool::from_value(cx, &value, false, ());
		assert!(!result.unwrap());

		let value = Value::string(cx, "spider");
		let result = bool::from_value(cx, &value, true, ());
		assert!(result.is_err());
		let result = bool::from_value(cx, &value, false, ());
		assert!(result.unwrap());

		let value = Value::undefined(cx);
		let result = bool::from_value(cx, &value, true, ());
		assert!(result.is_err());
		let result = bool::from_value(cx, &value, false, ());
		assert!(!result.unwrap());

		let value = Value::null(cx);
		let result = bool::from_value(cx, &value, true, ());
		assert!(result.is_err());
		let result = bool::from_value(cx, &value, false, ());
		assert!(!result.unwrap());

		let object = Object::new(cx);
		let value = object.as_value(cx);
		let result = bool::from_value(cx, &value, true, ());
		assert!(result.is_err());
		let result = bool::from_value(cx, &value, false, ());
		assert!(result.unwrap());

		let array = Array::new(cx);
		let value = array.as_value(cx);
		let result = bool::from_value(cx, &value, true, ());
		assert!(result.is_err());
		let result = bool::from_value(cx, &value, false, ());
		assert!(result.unwrap());
	}

	#[test]
	fn integer() {
		let rt = TestRuntime::new();
		let cx = &rt.cx;

		let value = Value::bool(cx, true);
		let result = i32::from_value(cx, &value, true, ConversionBehavior::EnforceRange);
		assert!(result.is_err());
		let result = i32::from_value(cx, &value, false, ConversionBehavior::EnforceRange);
		assert_eq!(result.unwrap(), 1);

		let value = Value::i32(cx, 255);
		let result = u8::from_value(cx, &value, true, ConversionBehavior::EnforceRange);
		assert_eq!(result.unwrap(), 255);

		let value = Value::string(cx, "spider");
		let result = u16::from_value(cx, &value, true, ConversionBehavior::EnforceRange);
		assert!(result.is_err());
		let result = u16::from_value(cx, &value, false, ConversionBehavior::EnforceRange);
		assert!(result.is_err());

		let value = Value::string(cx, "-64");
		let result = i64::from_value(cx, &value, true, ConversionBehavior::EnforceRange);
		assert!(result.is_err());
		let result = i64::from_value(cx, &value, false, ConversionBehavior::EnforceRange);
		assert_eq!(result.unwrap(), -64);

		let value = Value::undefined(cx);
		let result = i64::from_value(cx, &value, true, ConversionBehavior::EnforceRange);
		assert!(result.is_err());
		let result = i64::from_value(cx, &value, false, ConversionBehavior::EnforceRange);
		assert!(result.is_err());

		let value = Value::null(cx);
		let result = i64::from_value(cx, &value, true, ConversionBehavior::EnforceRange);
		assert!(result.is_err());
		let result = i64::from_value(cx, &value, false, ConversionBehavior::EnforceRange);
		assert_eq!(result.unwrap(), 0);

		let object = Object::new(cx);
		let value = object.as_value(cx);
		let result = u32::from_value(cx, &value, true, ConversionBehavior::EnforceRange);
		assert!(result.is_err());
		let result = u32::from_value(cx, &value, false, ConversionBehavior::EnforceRange);
		assert!(result.is_err());
	}

	#[test]
	fn string() {
		let rt = TestRuntime::new();
		let cx = &rt.cx;

		let value = Value::bool(cx, false);
		let result = String::from_value(cx, &value, true, ());
		assert!(result.is_err());
		let result = String::from_value(cx, &value, false, ());
		assert_eq!(&result.unwrap(), "false");

		let value = Value::f64(cx, 1.5);
		let result = String::from_value(cx, &value, true, ());
		assert!(result.is_err());
		let result = String::from_value(cx, &value, false, ());
		assert_eq!(&result.unwrap(), "1.5");

		let value = Value::string(cx, "spider");
		let result = String::from_value(cx, &value, true, ());
		assert_eq!(&result.unwrap(), "spider");

		let value = Value::undefined(cx);
		let result = String::from_value(cx, &value, true, ());
		assert!(result.is_err());
		let result = String::from_value(cx, &value, false, ());
		assert_eq!(&result.unwrap(), "undefined");

		let value = Value::null(cx);
		let result = String::from_value(cx, &value, true, ());
		assert!(result.is_err());
		let result = String::from_value(cx, &value, false, ());
		assert_eq!(&result.unwrap(), "null");

		let object = Object::new(cx);
		let value = object.as_value(cx);
		let result = String::from_value(cx, &value, true, ());
		assert!(result.is_err());
		let result = String::from_value(cx, &value, false, ());
		assert_eq!(&result.unwrap(), "[object Object]");
	}

	#[test]
	fn object() {
		let rt = TestRuntime::new();
		let cx = &rt.cx;

		let value = Value::bool(cx, false);
		let result = Object::from_value(cx, &value, true, ());
		assert!(result.is_err());
		let result = Object::from_value(cx, &value, false, ());
		assert!(result.is_err());

		let value = Value::f64(cx, 144.0);
		let result = Object::from_value(cx, &value, true, ());
		assert!(result.is_err());
		let result = Object::from_value(cx, &value, false, ());
		assert!(result.is_err());

		let value = Value::string(cx, "spider");
		let result = Object::from_value(cx, &value, true, ());
		assert!(result.is_err());
		let result = Object::from_value(cx, &value, false, ());
		assert!(result.is_err());

		let value = Value::undefined(cx);
		let result = Object::from_value(cx, &value, true, ());
		assert!(result.is_err());
		let result = Object::from_value(cx, &value, false, ());
		assert!(result.is_err());

		let value = Value::null(cx);
		let result = Object::from_value(cx, &value, true, ());
		assert!(result.is_err());
		let result = Object::from_value(cx, &value, false, ());
		assert!(result.is_err());

		let object = Object::new(cx);
		let value = object.as_value(cx);
		let result = Object::from_value(cx, &value, true, ());
		assert!(result.is_ok());

		let array = Array::new(cx);
		let value = array.as_value(cx);
		let result = Array::from_value(cx, &value, true, ());
		assert!(result.is_ok());

		let timestamp = Utc.timestamp_millis_opt(Utc::now().timestamp_millis()).unwrap();
		let date = Date::from_date(cx, timestamp);
		let value = date.as_value(cx);
		let result = Date::from_value(cx, &value, true, ());
		assert_eq!(result.unwrap().to_date(cx).unwrap(), timestamp);

		let promise = Promise::new(cx);
		let value = promise.as_value(cx);
		let result = Promise::from_value(cx, &value, true, ());
		assert!(result.is_ok());
	}

	#[test]
	fn option() {
		type Opt = Option<bool>;

		let rt = TestRuntime::new();
		let cx = &rt.cx;

		let value = Value::bool(cx, true);
		let result = Opt::from_value(cx, &value, true, ());
		assert_eq!(result.unwrap(), Some(true));

		let value = Value::undefined(cx);
		let result = Opt::from_value(cx, &value, true, ());
		assert_eq!(result.unwrap(), None);

		let value = Value::null(cx);
		let result = Opt::from_value(cx, &value, true, ());
		assert_eq!(result.unwrap(), None);
	}

	#[test]
	fn vec() {
		let rt = TestRuntime::new();
		let cx = &rt.cx;

		let int_vec = vec![1, 256, -65536, 2147483647];
		let mut root = RootableVec::new_unrooted();
		let vec = RootedVec::from_iter(&mut root, int_vec.iter().map(|i| Int32Value(*i)));
		let array = Array::from_rooted_vec(cx, &vec);
		let value = array.as_value(cx);

		let result = <Vec<i32>>::from_value(cx, &value, true, ConversionBehavior::EnforceRange);
		assert_eq!(result.unwrap(), int_vec);
	}
}
