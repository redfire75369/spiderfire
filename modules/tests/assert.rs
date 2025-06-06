/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::path::Path;

use ion::module::Module;
use ion::{Context, Error, Exception, Function, Object, Value};
use modules::Assert;
use mozjs::jsapi::JSFunctionSpec;
use mozjs::rust::{JSEngine, Runtime as RustRuntime};
use runtime::config::{CONFIG, Config, LogLevel};
use runtime::module::Loader;
use runtime::{Runtime, RuntimeBuilder};

const OK: (&str, &str) = ("ok", include_str!("scripts/assert/ok.js"));
const EQUALS: (&str, &str) = ("equals", include_str!("scripts/assert/equals.js"));
const THROWS: (&str, &str) = ("throws", include_str!("scripts/assert/throws.js"));
const FAIL: (&str, &str) = ("fail", include_str!("scripts/assert/fail.js"));

const EXCEPTION_STRING: &str = "_spidermonkey_exception_";

#[tokio::test]
async fn assert() {
	CONFIG.set(Config::default().log_level(LogLevel::Debug)).unwrap();

	let engine = JSEngine::init().unwrap();
	let rt = RustRuntime::new(engine.handle());

	let cx = &mut Context::from_runtime(&rt);
	let rt = RuntimeBuilder::new()
		.modules(Loader::default())
		.standard_modules(Assert)
		.microtask_queue()
		.build(cx);

	eval_module(&rt, rt.cx(), OK).await;
	eval_module(&rt, rt.cx(), EQUALS).await;
	eval_module(&rt, rt.cx(), THROWS).await;
	eval_module(&rt, rt.cx(), FAIL).await;
}

pub async fn eval_module(rt: &Runtime<'_>, cx: &Context, test: (&str, &str)) {
	let (test, script) = test;
	let filename = format!("{test}.js");
	let path = format!("./tests/scripts/assert/{test}.js");
	let error = format!("Assertion Failed: assert.{test}");

	let result = Module::compile_and_evaluate(cx, &filename, Some(Path::new(&path)), script);
	assert!(result.is_ok(), "Exception was thrown in: {filename}");

	let (_, promise) = result.unwrap();
	assert!(promise.is_some());

	let on_rejected = Function::from_spec(cx, &ON_REJECTED);
	promise.unwrap().add_reactions_native(cx, None, Some(on_rejected));

	rt.run_event_loop().await.unwrap();

	let exception = rt.global().get(rt.cx(), EXCEPTION_STRING).unwrap().unwrap();
	let exception = Exception::from_value(cx, &exception).unwrap();
	match &exception {
		Exception::Error(Error { message, .. }) => {
			assert_eq!(message, &error, "{filename}: {exception:#?}");
		}
		_ => {
			panic!("Exception was not an Error");
		}
	}
}

#[ion::js_fn]
fn on_rejected(cx: &Context, value: Value) {
	let global = Object::global(cx);
	global.set(cx, EXCEPTION_STRING, &value);
	Exception::clear(cx);
}

static ON_REJECTED: JSFunctionSpec = ion::function_spec!(on_rejected, c"onRejected", 0);
