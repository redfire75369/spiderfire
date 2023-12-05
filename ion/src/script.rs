/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::path::Path;

use mozjs::jsapi::{Compile, Heap, JS_ExecuteScript, JSScript};
use mozjs::jsval::UndefinedValue;
use mozjs::rust::{CompileOptionsWrapper, transform_u16_to_source_text};

use crate::{Context, ErrorReport, Root, Value};

#[derive(Debug)]
pub struct Script {
	script: Root<Box<Heap<*mut JSScript>>>,
}

impl Script {
	/// Compiles a script with a given filename and returns the compiled script.
	/// Returns [Err] when script compilation fails.
	pub fn compile(cx: &Context, path: &Path, script: &str) -> Result<Script, ErrorReport> {
		let script: Vec<u16> = script.encode_utf16().collect();
		let mut source = transform_u16_to_source_text(script.as_slice());
		let options = unsafe { CompileOptionsWrapper::new(cx.as_ptr(), path.to_str().unwrap(), 1) };

		let script = unsafe { Compile(cx.as_ptr(), options.ptr, &mut source) };

		if !script.is_null() {
			Ok(Script { script: cx.root(script) })
		} else {
			Err(ErrorReport::new_with_exception_stack(cx).unwrap())
		}
	}

	/// Evaluates a script and returns its return value.
	/// Returns [Err] when an exception occurs during script evaluation.
	pub fn evaluate(&self, cx: &Context) -> Result<Value, ErrorReport> {
		rooted!(in(cx.as_ptr()) let mut rval = UndefinedValue());
		if unsafe { JS_ExecuteScript(cx.as_ptr(), self.script.handle().into(), rval.handle_mut().into()) } {
			Ok(Value::from(cx.root(rval.get())))
		} else {
			Err(ErrorReport::new_with_exception_stack(cx).unwrap())
		}
	}

	/// Compiles and evaluates a script with a given filename, and returns its return value.
	/// Returns [Err] when script compilation fails or an exception occurs during script evaluation.
	pub fn compile_and_evaluate(cx: &Context, path: &Path, script: &str) -> Result<Value, ErrorReport> {
		match Script::compile(cx, path, script) {
			Ok(s) => s.evaluate(cx),
			Err(e) => Err(e),
		}
	}
}

impl From<Root<Box<Heap<*mut JSScript>>>> for Script {
	fn from(script: Root<Box<Heap<*mut JSScript>>>) -> Script {
		Script { script }
	}
}
