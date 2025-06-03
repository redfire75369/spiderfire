/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

use mozjs::glue::JS_GetRegExpFlags;
use mozjs::jsapi::{
	CheckRegExpSyntax, ExecuteRegExp, ExecuteRegExpNoStatics, GetRegExpSource, JSObject, NewUCRegExpObject,
	ObjectIsRegExp, RegExpFlags as REFlags,
};
use mozjs::rooted;

use crate::flags::RegExpFlags;
use crate::{Context, Local, Object, Value};

#[derive(Debug)]
pub struct RegExp<'r> {
	re: Local<'r, *mut JSObject>,
}

impl<'r> RegExp<'r> {
	/// Creates a new [RegExp] object with the given source and flags.
	pub fn new(cx: &'r Context, source: &str, flags: RegExpFlags) -> Option<RegExp<'r>> {
		let source: Vec<u16> = source.encode_utf16().collect();
		let regexp = unsafe { NewUCRegExpObject(cx.as_ptr(), source.as_ptr(), source.len(), flags.into()) };
		NonNull::new(regexp).map(|re| RegExp { re: cx.root(re.as_ptr()) })
	}

	/// Creates a [RegExp] from an object.
	/// Returns [None] if it is not a [RegExp].
	pub fn from(cx: &Context, object: Local<'r, *mut JSObject>) -> Option<RegExp<'r>> {
		if RegExp::is_regexp(cx, &object) {
			Some(RegExp { re: object })
		} else {
			None
		}
	}

	/// Creates a [RegExp] from an object.
	///
	/// ### Safety
	/// Object must be a [RegExp].
	pub unsafe fn from_unchecked(object: Local<'r, *mut JSObject>) -> RegExp<'r> {
		RegExp { re: object }
	}

	pub fn source<'cx>(&self, cx: &'cx Context) -> crate::String<'cx> {
		crate::String::from(cx.root(unsafe { GetRegExpSource(cx.as_ptr(), self.handle().into()) }))
	}

	pub fn flags(&self, cx: &Context) -> RegExpFlags {
		let mut flags = REFlags { flags_: 0 };
		unsafe {
			JS_GetRegExpFlags(cx.as_ptr(), self.handle().into(), &mut flags);
		}
		flags.into()
	}

	pub fn to_string(&self, cx: &Context) -> crate::Result<String> {
		Ok(format!("/{}/{}", self.source(cx).to_owned(cx)?, self.flags(cx)))
	}

	pub fn execute_test(&self, cx: &Context, string: &str, index: &mut usize) -> bool {
		let mut rval = Value::null(cx);
		self.execute(cx, string, index, true, &mut rval, true) && rval.handle().to_boolean()
	}

	pub fn execute_test_no_static(&self, cx: &Context, string: &str, index: &mut usize) -> bool {
		let mut rval = Value::null(cx);
		self.execute(cx, string, index, true, &mut rval, false) && rval.handle().to_boolean()
	}

	pub fn execute_match<'cx>(&self, cx: &'cx Context, string: &str, index: &mut usize) -> Option<Value<'cx>> {
		let mut rval = Value::null(cx);
		self.execute(cx, string, index, false, &mut rval, true).then_some(rval)
	}

	pub fn execute_match_no_static<'cx>(
		&self, cx: &'cx Context, string: &str, index: &mut usize,
	) -> Option<Value<'cx>> {
		let mut rval = Value::null(cx);
		self.execute(cx, string, index, false, &mut rval, false).then_some(rval)
	}

	fn execute<'cx>(
		&self, cx: &'cx Context, string: &str, index: &mut usize, test: bool, rval: &mut Value<'cx>, with_static: bool,
	) -> bool {
		let string: Vec<u16> = string.encode_utf16().collect();
		if with_static {
			let global = Object::global(cx);
			unsafe {
				ExecuteRegExp(
					cx.as_ptr(),
					global.handle().into(),
					self.handle().into(),
					string.as_ptr(),
					string.len(),
					index,
					test,
					rval.handle_mut().into(),
				)
			}
		} else {
			unsafe {
				ExecuteRegExpNoStatics(
					cx.as_ptr(),
					self.handle().into(),
					string.as_ptr(),
					string.len(),
					index,
					test,
					rval.handle_mut().into(),
				)
			}
		}
	}

	pub fn check_syntax<'cx>(cx: &'cx Context, source: &str, flags: RegExpFlags) -> Result<(), Value<'cx>> {
		let source: Vec<u16> = source.encode_utf16().collect();
		let mut error = Value::undefined(cx);
		let check = unsafe {
			CheckRegExpSyntax(
				cx.as_ptr(),
				source.as_ptr(),
				source.len(),
				flags.into(),
				error.handle_mut().into(),
			)
		};
		if check {
			if error.handle().is_undefined() {
				Ok(())
			} else {
				Err(error)
			}
		} else {
			Err(error)
		}
	}

	/// Checks if a [raw object](*mut JSObject) is a regexp.
	pub fn is_regexp_raw(cx: &Context, object: *mut JSObject) -> bool {
		rooted!(in(cx.as_ptr()) let object = object);
		let mut is_regexp = false;
		(unsafe { ObjectIsRegExp(cx.as_ptr(), object.handle().into(), &mut is_regexp) }) && is_regexp
	}

	/// Checks if an object is a regexp.
	pub fn is_regexp(cx: &Context, object: &Local<*mut JSObject>) -> bool {
		let mut is_regexp = false;
		(unsafe { ObjectIsRegExp(cx.as_ptr(), object.handle().into(), &mut is_regexp) }) && is_regexp
	}
}

impl<'r> Deref for RegExp<'r> {
	type Target = Local<'r, *mut JSObject>;

	fn deref(&self) -> &Self::Target {
		&self.re
	}
}

impl DerefMut for RegExp<'_> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.re
	}
}
