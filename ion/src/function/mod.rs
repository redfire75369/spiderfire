/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::any::Any;
use std::mem::forget;
use std::thread::Result;

pub use arguments::{Accessor, Arguments, FromArgument};
pub use closure::{Closure, ClosureOnce};
pub use function::{Function, NativeFunction};
use mozjs::conversions::ConversionBehavior;

use crate::conversions::{FromValue, ToValue as _};
use crate::{Context, Error, Object, ResultExc, ThrowException as _, Value};

mod arguments;
mod closure;
mod function;

#[doc(hidden)]
pub fn __handle_native_function_result(cx: &Context, result: Result<ResultExc<()>>) -> bool {
	match result {
		Ok(Ok(_)) => true,
		Ok(Err(error)) => {
			error.throw(cx);
			false
		}
		Err(unwind_error) => handle_unwind_error(cx, unwind_error),
	}
}

#[doc(hidden)]
pub fn __handle_native_constructor_result(
	cx: &Context, result: Result<ResultExc<()>>, this: &Object, rval: &mut Value,
) -> bool {
	match result {
		Ok(Ok(_)) => {
			this.to_value(cx, rval);
			true
		}
		Ok(Err(error)) => {
			error.throw(cx);
			false
		}
		Err(unwind_error) => handle_unwind_error(cx, unwind_error),
	}
}

fn handle_unwind_error(cx: &Context, unwind_error: Box<dyn Any + Send>) -> bool {
	match unwind_error.downcast::<String>() {
		Ok(unwind) => Error::new(*unwind, None).throw(cx),
		Err(unwind_error) => {
			if let Some(unwind) = unwind_error.downcast_ref::<&'static str>() {
				Error::new(*unwind, None).throw(cx);
			} else {
				Error::new("Unknown Panic Occurred", None).throw(cx);
				forget(unwind_error);
			}
		}
	}
	false
}

/// Helper type for optional arguments.
pub struct Opt<T>(pub Option<T>);

/// Helper type for rest/spread/variable arguments.
pub struct Rest<T>(pub Box<[T]>);

pub type VarArgs<T> = Rest<T>;

/// Helper type for strict arguments.
pub struct Strict<T>(pub T);

impl<'cx, T: FromValue<'cx>> FromValue<'cx> for Strict<T> {
	type Config = T::Config;

	fn from_value(cx: &'cx Context, value: &Value, _: bool, config: Self::Config) -> crate::Result<Strict<T>> {
		T::from_value(cx, value, true, config).map(Strict)
	}
}

/// Helper type for integer arguments that wraps.
#[derive(Clone, Copy, Debug, Default)]
pub struct Wrap<T>(pub T);

impl<'cx, T: FromValue<'cx, Config = ConversionBehavior>> FromValue<'cx> for Wrap<T> {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, strict: bool, _: ()) -> crate::Result<Wrap<T>> {
		T::from_value(cx, value, strict, ConversionBehavior::Default).map(Wrap)
	}
}

/// Helper type for integer arguments that enforces the range.
#[derive(Clone, Copy, Debug, Default)]
pub struct Enforce<T>(pub T);

impl<'cx, T: FromValue<'cx, Config = ConversionBehavior>> FromValue<'cx> for Enforce<T> {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, strict: bool, _: ()) -> crate::Result<Enforce<T>> {
		T::from_value(cx, value, strict, ConversionBehavior::EnforceRange).map(Enforce)
	}
}

/// Helper type for integer arguments that clamps.
#[derive(Clone, Copy, Debug, Default)]
pub struct Clamp<T>(pub T);

impl<'cx, T: FromValue<'cx, Config = ConversionBehavior>> FromValue<'cx> for Clamp<T> {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, strict: bool, _: ()) -> crate::Result<Clamp<T>> {
		T::from_value(cx, value, strict, ConversionBehavior::Clamp).map(Clamp)
	}
}
