/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use mozjs::jsapi::{CurrentGlobalOrNull, JS_DefineFunctions, JS_NewPlainObject, JSFunctionSpec, SameValue, Value};

use ion::{IonContext, IonResult};
use ion::error::IonError;
use ion::functions::function::IonFunction;
use ion::objects::object::IonObject;
use runtime::modules::Module;

fn assert_internal(message: Option<String>) -> IonResult<()> {
	Err(IonError::Error(match message {
		Some(msg) => format!("Assertion Failed: {}", msg),
		None => String::from("Assertion Failed"),
	}))
}

#[js_fn]
fn ok(assertion: Option<bool>, message: Option<String>) -> IonResult<()> {
	if let Some(true) = assertion {
		Ok(())
	} else {
		assert_internal(message)
	}
}

#[js_fn]
unsafe fn equals(cx: IonContext, actual: Value, expected: Value, message: Option<String>) -> IonResult<()> {
	let mut same = false;
	rooted!(in(cx) let actual = actual);
	rooted!(in(cx) let expected = expected);
	if SameValue(cx, actual.handle().into(), expected.handle().into(), &mut same) {
		if same {
			Ok(())
		} else {
			assert_internal(message)
		}
	} else {
		Err(IonError::None)
	}
}

#[js_fn]
unsafe fn throws(cx: IonContext, func: IonFunction, message: Option<String>) -> IonResult<()> {
	if func.call_with_vec(cx, IonObject::from(CurrentGlobalOrNull(cx)), Vec::new()).is_err() {
		assert_internal(message)
	} else {
		Ok(())
	}
}

#[js_fn]
fn fail(message: Option<String>) -> IonResult<()> {
	assert_internal(message)
}

const FUNCTIONS: &[JSFunctionSpec] = &[
	function_spec!(ok, 0),
	function_spec!(equals, 2),
	function_spec!(throws, 1),
	function_spec!(fail, 0),
	JSFunctionSpec::ZERO,
];

#[derive(Default)]
pub struct Assert;

impl Module for Assert {
	const NAME: &'static str = "assert";
	const SOURCE: &'static str = include_str!("assert.js");

	unsafe fn module(cx: IonContext) -> Option<IonObject> {
		rooted!(in(cx) let assert = JS_NewPlainObject(cx));
		if JS_DefineFunctions(cx, assert.handle().into(), FUNCTIONS.as_ptr()) {
			Some(IonObject::from(assert.get()))
		} else {
			None
		}
	}
}
