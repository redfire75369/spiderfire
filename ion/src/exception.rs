/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */
use std::ptr;

use mozjs::conversions::ConversionBehavior;
use mozjs::jsapi::{
	ESClass, ExceptionStack, ExceptionStackBehavior, ExceptionStackOrNull, GetPendingExceptionStack,
	IdentifyStandardInstance, JS_ClearPendingException, JS_GetPendingException, JS_IsExceptionPending,
	JS_SetPendingException, Rooted,
};
use mozjs::jsval::{JSVal, ObjectValue, UndefinedValue};
#[cfg(feature = "sourcemap")]
use sourcemap::SourceMap;

use crate::conversions::{FromValue, ToValue};
use crate::format::{Config, NEWLINE, format_value};
use crate::stack::Location;
use crate::{Context, Error, ErrorKind, Object, Result, Stack, Value};

pub trait ThrowException {
	fn throw(&self, cx: &Context);
}

/// Represents an exception in the JS Runtime.
/// The exception can be an [Error], or any [Value].
#[derive(Clone, Debug)]
pub enum Exception {
	Error(Error),
	Other(JSVal),
}

impl Exception {
	/// Gets an [Exception] from the runtime and clears the pending exception.
	/// Returns [None] if there is no pending exception.
	pub fn new(cx: &Context) -> Result<Option<Exception>> {
		unsafe {
			if JS_IsExceptionPending(cx.as_ptr()) {
				let mut exception = Value::undefined(cx);
				if JS_GetPendingException(cx.as_ptr(), exception.handle_mut().into()) {
					let exception = Exception::from_value(cx, &exception)?;
					Exception::clear(cx);
					Ok(Some(exception))
				} else {
					Ok(None)
				}
			} else {
				Ok(None)
			}
		}
	}

	/// Converts a [Value] into an [Exception].
	pub fn from_value<'cx>(cx: &'cx Context, value: &Value<'cx>) -> Result<Exception> {
		if value.handle().is_object() {
			let object = value.to_object(cx);
			Exception::from_object(cx, &object)
		} else {
			Ok(Exception::Other(value.get()))
		}
	}

	/// Converts an [Object] into an [Exception].
	/// If the object is an error object, it is parsed as an [Error].
	pub fn from_object<'cx>(cx: &'cx Context, exception: &Object<'cx>) -> Result<Exception> {
		unsafe {
			let handle = exception.handle();
			if exception.get_builtin_class(cx) == ESClass::Error {
				let message = String::from_value(cx, &exception.get(cx, "message")?.unwrap(), true, ()).unwrap();
				let file: String = exception.get_as(cx, "fileName", true, ())?.unwrap();
				let lineno: u32 = exception.get_as(cx, "lineNumber", true, ConversionBehavior::Clamp)?.unwrap();
				let column: u32 = exception.get_as(cx, "columnNumber", true, ConversionBehavior::Clamp)?.unwrap();

				let location = Location { file, lineno, column };
				let kind = ErrorKind::from_proto_key(IdentifyStandardInstance(handle.get()));
				let error = Error {
					kind,
					message: message.into(),
					location: Some(location),
					object: Some(handle.get()),
				};
				Ok(Exception::Error(error))
			} else {
				Ok(Exception::Other(ObjectValue(handle.get())))
			}
		}
	}

	/// Clears the pending exception within the runtime.
	pub fn clear(cx: &Context) {
		unsafe { JS_ClearPendingException(cx.as_ptr()) };
	}

	/// Checks if an exception is pending in the runtime.
	pub fn is_pending(cx: &Context) -> bool {
		unsafe { JS_IsExceptionPending(cx.as_ptr()) }
	}

	/// Converts [Exception] to an [Error]
	/// Returns [Error::none()](Error::none) if the exception is not an [Error].
	pub fn to_error(&self) -> Error {
		match self {
			Exception::Error(error) => error.clone(),
			Exception::Other(_) => Error::none(),
		}
	}

	/// If the [Exception] is an [Error], the error location is mapped according to the given [SourceMap].
	#[cfg(feature = "sourcemap")]
	pub fn transform_with_sourcemap(&mut self, sourcemap: &SourceMap) {
		if let Exception::Error(Error { location: Some(location), .. }) = self {
			if let Some(token) = sourcemap.lookup_token(location.lineno - 1, location.column - 1) {
				location.lineno = token.get_src_line() + 1;
				location.column = token.get_src_col() + 1;
			}
		}
	}

	/// Formats the [Exception] as an error message.
	pub fn format(&self, cx: &Context) -> String {
		match self {
			Exception::Error(error) => format!("Uncaught {}", error.format()),
			Exception::Other(value) => {
				format!(
					"Uncaught Exception - {}",
					format_value(cx, Config::default(), &cx.root(*value).into())
				)
			}
		}
	}
}

impl ThrowException for Exception {
	fn throw(&self, cx: &Context) {
		match self {
			Exception::Error(error) => {
				if let Error { object: Some(object), .. } = error {
					let exception = Value::from(cx.root(ObjectValue(*object)));
					unsafe {
						JS_SetPendingException(
							cx.as_ptr(),
							exception.handle().into(),
							ExceptionStackBehavior::DoNotCapture,
						)
					}
				} else {
					error.throw(cx);
				}
			}
			Exception::Other(value) => {
				let value = Value::from(cx.root(*value));
				unsafe { JS_SetPendingException(cx.as_ptr(), value.handle().into(), ExceptionStackBehavior::Capture) }
			}
		}
	}
}

impl<'cx> ToValue<'cx> for Exception {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		match self {
			Exception::Error(error) => error.to_value(cx, value),
			Exception::Other(other) => value.handle_mut().set(*other),
		}
	}
}

impl<E: Into<Error>> From<E> for Exception {
	fn from(error: E) -> Exception {
		Exception::Error(error.into())
	}
}

/// Represents an error report, containing an exception and optionally its [stacktrace](Stack).
#[derive(Clone, Debug)]
pub struct ErrorReport {
	pub exception: Exception,
	pub stack: Option<Stack>,
}

impl ErrorReport {
	/// Creates a new [ErrorReport] with an [Exception] from the runtime and clears the pending exception.
	/// Returns [None] if there is no pending exception.
	pub fn new(cx: &Context) -> Result<Option<ErrorReport>> {
		Ok(Exception::new(cx)?.map(|exception| ErrorReport { exception, stack: None }))
	}

	/// Creates a new [ErrorReport] with an [Exception] and [Error]'s exception stack.
	/// Returns [None] if there is no pending exception.
	pub fn new_with_error_stack(cx: &Context) -> Result<Option<ErrorReport>> {
		Ok(ErrorReport::new(cx)?.map(|report| ErrorReport::from_exception_with_error_stack(cx, report.exception)))
	}

	/// Creates a new [ErrorReport] with an [Exception] and exception stack from the runtime.
	/// Returns [None] if there is no pending exception.
	pub fn new_with_exception_stack(cx: &Context) -> Result<Option<ErrorReport>> {
		unsafe {
			if JS_IsExceptionPending(cx.as_ptr()) {
				let mut exception_stack = ExceptionStack {
					exception_: Rooted::new_unrooted(UndefinedValue()),
					stack_: Rooted::new_unrooted(ptr::null_mut()),
				};

				if GetPendingExceptionStack(cx.as_ptr(), &mut exception_stack) {
					let exception = Value::from(cx.root(exception_stack.exception_.data));
					let exception = Exception::from_value(cx, &exception)?;
					let stack = Stack::from_object(cx, exception_stack.stack_.data);
					Exception::clear(cx);
					Ok(Some(ErrorReport { exception, stack }))
				} else {
					Ok(None)
				}
			} else {
				Ok(None)
			}
		}
	}

	/// Creates an [ErrorReport] from an existing [Exception] and optionally a [Stack].
	pub fn from<S: Into<Option<Stack>>>(exception: Exception, stack: S) -> ErrorReport {
		ErrorReport { exception, stack: stack.into() }
	}

	/// Creates an [ErrorReport] from an existing [Exception], with the [Error]'s exception stack.
	pub fn from_exception_with_error_stack(cx: &Context, exception: Exception) -> ErrorReport {
		let stack = if let Exception::Error(Error { object: Some(object), .. }) = exception {
			unsafe {
				rooted!(in(cx.as_ptr()) let exc = object);
				Stack::from_object(cx, ExceptionStackOrNull(exc.handle().into()))
			}
		} else {
			None
		};
		ErrorReport { exception, stack }
	}

	/// Transforms the location of the [Exception] and the [Stack] if it exists, according to the given [SourceMap].
	#[cfg(feature = "sourcemap")]
	pub fn transform_with_sourcemap(&mut self, sourcemap: &SourceMap) {
		self.exception.transform_with_sourcemap(sourcemap);
		if let Some(stack) = &mut self.stack {
			stack.transform_with_sourcemap(sourcemap)
		}
	}

	/// Formats the [ErrorReport] as a string for printing.
	pub fn format(&self, cx: &Context) -> String {
		let mut string = self.exception.format(cx);
		if let Some(stack) = &self.stack {
			if !stack.is_empty() {
				string.push_str(NEWLINE);
				string.push_str(&stack.format());
			}
		}
		string
	}
}
