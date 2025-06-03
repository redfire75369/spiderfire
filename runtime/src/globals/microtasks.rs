/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use ion::flags::PropertyFlags;
use ion::{Context, Error, Function, Object, Result, function_spec, js_fn};
use mozjs::jsapi::JSFunctionSpec;

use crate::ContextExt;
use crate::event_loop::microtasks::Microtask;

#[js_fn]
fn queue_microtask(cx: &Context, callback: Function) -> Result<()> {
	let event_loop = unsafe { &mut cx.get_private().event_loop };
	if let Some(queue) = &mut event_loop.microtasks {
		queue.enqueue(cx, Microtask::User(callback.get()));
		Ok(())
	} else {
		Err(Error::new("Microtask Queue has not been initialised.", None))
	}
}

const FUNCTION: JSFunctionSpec = function_spec!(queue_microtask, c"queueMicrotask", 0);

pub fn define(cx: &Context, global: &Object) -> bool {
	global.define_as(
		cx,
		"queueMicrotask",
		&Function::from_spec(cx, &FUNCTION),
		PropertyFlags::CONSTANT_ENUMERATED,
	)
}
