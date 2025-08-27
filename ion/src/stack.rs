/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::fmt::{Display, Formatter, Write as _};
use std::mem::MaybeUninit;
use std::ptr::NonNull;
use std::{fmt, ptr};

use mozjs::conversions::jsstr_to_string;
use mozjs::jsapi::{
	BuildStackString, CaptureCurrentStack, JS_StackCapture_AllFrames, JS_StackCapture_MaxFrames, JSObject, JSString,
	StackFormat,
};
use mozjs::rooted;
#[cfg(feature = "sourcemap")]
use swc_sourcemap::SourceMap;

use crate::format::{INDENT, NEWLINE};
use crate::utils::normalise_path;
use crate::{Context, Object};

/// Represents a location in a source file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
	pub file: String,
	pub lineno: u32,
	pub column: u32,
}

/// Represents a single stack record of a [stacktrace](Stack).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackRecord {
	pub function: Option<String>,
	pub location: Location,
}

/// Represents a stacktrace.
///
/// Holds a stack object if cretead from an [Object].
#[derive(Clone, Debug)]
pub struct Stack {
	pub records: Vec<StackRecord>,
	pub object: Option<*mut JSObject>,
}

impl Location {
	/// Transforms a [Location], according to the given [SourceMap].
	#[cfg(feature = "sourcemap")]
	pub fn transform_with_sourcemap(&mut self, sourcemap: &SourceMap) {
		if self.lineno != 0
			&& self.column != 0
			&& let Some(token) = sourcemap.lookup_token(self.lineno - 1, self.column - 1)
		{
			self.lineno = token.get_src_line() + 1;
			self.column = token.get_src_col() + 1;
		}
	}
}

impl StackRecord {
	/// Transforms a [StackRecord], according to the given [SourceMap].
	#[cfg(feature = "sourcemap")]
	pub fn transform_with_sourcemap(&mut self, sourcemap: &SourceMap) {
		self.location.transform_with_sourcemap(sourcemap);
	}
}

impl Display for StackRecord {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		f.write_str(self.function.as_deref().unwrap_or(""))?;
		f.write_char('@')?;
		f.write_str(&self.location.file)?;
		f.write_char(':')?;
		f.write_str(&self.location.lineno.to_string())?;
		f.write_char(':')?;
		f.write_str(&self.location.column.to_string())?;
		Ok(())
	}
}

impl Stack {
	/// Creates a [Stack] from a string.
	pub fn from_string(string: &str) -> Stack {
		let mut records = Vec::new();
		for line in string.lines() {
			let (function, line) = line.split_once('@').unwrap();
			let (line, column) = line.rsplit_once(':').unwrap();
			let (file, lineno) = line.rsplit_once(':').unwrap();

			let function = if function.is_empty() {
				None
			} else {
				Some(String::from(function))
			};
			let file = String::from(normalise_path(file).to_str().unwrap());
			let lineno = lineno.parse().unwrap();
			let column = column.parse().unwrap();

			records.push(StackRecord {
				function,
				location: Location { file, lineno, column },
			});
		}
		Stack { records, object: None }
	}

	/// Creates a [Stack] from an object.
	pub fn from_object(cx: &Context, stack: *mut JSObject) -> Option<Stack> {
		stack_to_string(cx, stack).as_deref().map(Stack::from_string).map(|mut s| {
			s.object = Some(stack);
			s
		})
	}

	/// Captures the [Stack] of the [Context].
	pub fn from_capture(cx: &Context) -> Option<Stack> {
		let stack = capture_stack(cx, None)?;
		Stack::from_object(cx, stack)
	}

	/// Returns `true` if the stack contains no [records](StackRecord)
	pub fn is_empty(&self) -> bool {
		self.records.is_empty()
	}

	/// Transforms a [Stack] with the given [SourceMap], by applying it to each of its [records](StackRecord).
	#[cfg(feature = "sourcemap")]
	pub fn transform_with_sourcemap(&mut self, sourcemap: &SourceMap) {
		for record in &mut self.records {
			record.transform_with_sourcemap(sourcemap);
		}
	}

	/// Formats the [Stack] as a String.
	pub fn format(&self) -> String {
		let mut string = String::new();
		for record in &self.records {
			string.push_str(INDENT);
			string.push_str(&record.to_string());
			string.push_str(NEWLINE);
		}
		string.pop();
		string
	}
}

impl Display for Stack {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		f.write_str(&self.format())
	}
}

fn capture_stack(cx: &Context, max_frames: Option<u32>) -> Option<*mut JSObject> {
	unsafe {
		let mut capture = MaybeUninit::uninit();
		match max_frames {
			None => JS_StackCapture_AllFrames(capture.as_mut_ptr()),
			Some(count) => JS_StackCapture_MaxFrames(count, capture.as_mut_ptr()),
		}
		let mut capture = capture.assume_init();

		let mut stack = Object::null(cx);
		let start_at = Object::null(cx);
		CaptureCurrentStack(
			cx.as_ptr(),
			stack.handle_mut().into(),
			&raw mut capture,
			start_at.handle().into(),
		)
		.then(|| stack.handle().get())
	}
}

fn stack_to_string(cx: &Context, stack: *mut JSObject) -> Option<String> {
	unsafe {
		rooted!(in(cx.as_ptr()) let stack = stack);
		rooted!(in(cx.as_ptr()) let mut string: *mut JSString);

		BuildStackString(
			cx.as_ptr(),
			ptr::null_mut(),
			stack.handle().into(),
			string.handle_mut().into(),
			0,
			StackFormat::SpiderMonkey,
		)
		.then(|| NonNull::new(string.get()).map(|str| jsstr_to_string(cx.as_ptr(), str)))
		.flatten()
	}
}
