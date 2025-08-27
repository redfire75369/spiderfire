/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use mozjs::jsapi::mozilla::{Range, RangedPtr};
use mozjs::jsapi::{
	BigInt as JSBigInt, BigIntFitsNumber, BigIntFromBool, BigIntFromInt64, BigIntFromUint64, BigIntIsInt64,
	BigIntIsNegative, BigIntIsUint64, BigIntToNumber, BigIntToString, NumberToBigInt, StringToBigInt1,
};

use crate::{Context, Exception, Local, String};

pub struct BigInt<'b> {
	bi: Local<'b, *mut JSBigInt>,
}

impl<'b> BigInt<'b> {
	/// Creates a [BigInt] from a boolean.
	pub fn from_bool(cx: &Context, boolean: bool) -> BigInt<'_> {
		BigInt::from(cx.root(unsafe { BigIntFromBool(cx.as_ptr(), boolean) }))
	}

	/// Creates a [BigInt] from a 64-bit signed integer.
	pub fn from_i64(cx: &Context, number: i64) -> BigInt<'_> {
		BigInt::from(cx.root(unsafe { BigIntFromInt64(cx.as_ptr(), number) }))
	}

	/// Creates a [BigInt] from a 64-bit unsigned integer.
	pub fn from_u64(cx: &Context, number: u64) -> BigInt<'_> {
		BigInt::from(cx.root(unsafe { BigIntFromUint64(cx.as_ptr(), number) }))
	}

	/// Creates a [BigInt] from a double.
	/// Returns an error if `number` is `NaN`, `Infinity`, `-Infinity` or contains a fractional component.
	pub fn from_f64(cx: &Context, number: f64) -> Result<BigInt<'_>, Exception> {
		let bi = unsafe { NumberToBigInt(cx.as_ptr(), number) };
		if !bi.is_null() {
			Ok(BigInt::from(cx.root(bi)))
		} else {
			Err(Exception::new(cx)?.unwrap())
		}
	}

	/// Creates a [BigInt] from a string.
	pub fn from_string(cx: &'b Context, string: &str) -> Result<BigInt<'b>, Option<Exception>> {
		let mut string: Vec<u16> = string.encode_utf16().collect();
		let range = string.as_mut_ptr_range();
		let chars = Range {
			mStart: RangedPtr {
				mPtr: range.start,
				#[cfg(feature = "debugmozjs")]
				mRangeStart: range.start,
				#[cfg(feature = "debugmozjs")]
				mRangeEnd: range.end,
				_phantom_0: PhantomData,
			},
			mEnd: RangedPtr {
				mPtr: range.end,
				#[cfg(feature = "debugmozjs")]
				mRangeStart: range.start,
				#[cfg(feature = "debugmozjs")]
				mRangeEnd: range.end,
				_phantom_0: PhantomData,
			},
			_phantom_0: PhantomData,
		};
		let bi = unsafe { StringToBigInt1(cx.as_ptr(), &raw const chars) };
		if !bi.is_null() {
			Ok(BigInt::from(cx.root(bi)))
		} else {
			Err(Exception::new(cx).map_err(|e| Some(Exception::Error(e)))?)
		}
	}

	/// Converts a [BigInt] to a 64-bit signed integer if possible.
	pub fn to_i64(&self) -> Option<i64> {
		let mut result = 0;
		unsafe { BigIntIsInt64(self.get(), &raw mut result).then_some(result) }
	}

	/// Converts a [BigInt] to a 64-bit unsigned integer if possible.
	pub fn to_u64(&self) -> Option<u64> {
		let mut result = 0;
		unsafe { BigIntIsUint64(self.get(), &raw mut result).then_some(result) }
	}

	/// Converts a [BigInt] to a double.
	/// Returns `Infinity` or `-Infinity` if it does not fit in a double.
	pub fn to_f64(&self) -> f64 {
		unsafe { BigIntToNumber(self.get()) }
	}

	/// Converts a [BigInt] to a double if it fits in a double.
	pub fn fits_f64(&self) -> Option<f64> {
		let mut result = 0.0;
		unsafe { BigIntFitsNumber(self.get(), &raw mut result).then_some(result) }
	}

	/// Converts a [BigInt] to a string.
	/// Returns `None` if the radix is not within the range (2..=36).
	pub fn to_string<'cx>(&self, cx: &'cx Context, radix: u8) -> Option<String<'cx>> {
		if !(2..=36).contains(&radix) {
			None
		} else {
			let string = unsafe { BigIntToString(cx.as_ptr(), self.handle().into(), radix) };
			Some(String::from(cx.root(string)))
		}
	}

	/// Checks if the [BigInt] is negative.
	pub fn is_negative(&self) -> bool {
		unsafe { BigIntIsNegative(self.get()) }
	}
}

impl<'b> From<Local<'b, *mut JSBigInt>> for BigInt<'b> {
	fn from(bi: Local<'b, *mut JSBigInt>) -> BigInt<'b> {
		BigInt { bi }
	}
}

impl<'o> Deref for BigInt<'o> {
	type Target = Local<'o, *mut JSBigInt>;

	fn deref(&self) -> &Self::Target {
		&self.bi
	}
}

impl DerefMut for BigInt<'_> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.bi
	}
}
