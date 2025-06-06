/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::time::Duration;

use ion::function::{Clamp, Enforce, Opt, Rest};
use ion::{Context, Error, Function, Object, Result, function_spec, js_fn};
use mozjs::jsapi::JSFunctionSpec;
use mozjs::jsval::JSVal;

use crate::ContextExt as _;
use crate::event_loop::macrotasks::{Macrotask, TimerMacrotask, UserMacrotask};

const MINIMUM_DELAY: i32 = 1;
const MINIMUM_DELAY_NESTED: i32 = 4;

fn set_timer(
	cx: &Context, callback: &Function, duration: Option<Clamp<i32>>, arguments: Box<[JSVal]>, repeat: bool,
) -> Result<u32> {
	let event_loop = unsafe { &mut cx.get_private().event_loop };
	if let Some(queue) = &mut event_loop.macrotasks {
		let minimum = if queue.nesting > 5 {
			MINIMUM_DELAY_NESTED
		} else {
			MINIMUM_DELAY
		};

		let duration = duration.map_or(minimum, |t| t.0.max(minimum));
		let timer = TimerMacrotask::new(
			callback,
			arguments,
			repeat,
			Duration::from_millis(u64::try_from(duration).unwrap()),
		);
		Ok(queue.enqueue(Macrotask::Timer(timer), None))
	} else {
		Err(Error::new("Macrotask Queue has not been initialised.", None))
	}
}

fn clear_timer(cx: &Context, id: Option<Enforce<u32>>) -> Result<()> {
	if let Some(id) = id {
		let event_loop = unsafe { &mut cx.get_private().event_loop };
		if let Some(queue) = &mut event_loop.macrotasks {
			queue.remove(id.0);
			Ok(())
		} else {
			Err(Error::new("Macrotask Queue has not been initialised.", None))
		}
	} else {
		Ok(())
	}
}

#[js_fn]
fn set_timeout(
	cx: &Context, callback: Function, Opt(duration): Opt<Clamp<i32>>, Rest(arguments): Rest<JSVal>,
) -> Result<u32> {
	set_timer(cx, &callback, duration, arguments, false)
}

#[js_fn]
fn set_interval(
	cx: &Context, callback: Function, Opt(duration): Opt<Clamp<i32>>, Rest(arguments): Rest<JSVal>,
) -> Result<u32> {
	set_timer(cx, &callback, duration, arguments, true)
}

#[js_fn]
fn clear_timeout(cx: &Context, Opt(id): Opt<Enforce<u32>>) -> Result<()> {
	clear_timer(cx, id)
}

#[js_fn]
fn clear_interval(cx: &Context, Opt(id): Opt<Enforce<u32>>) -> Result<()> {
	clear_timer(cx, id)
}

#[js_fn]
fn queue_macrotask(cx: &Context, callback: Function) -> Result<()> {
	let event_loop = unsafe { &mut cx.get_private().event_loop };
	if let Some(queue) = &mut event_loop.macrotasks {
		queue.enqueue(Macrotask::User(UserMacrotask::new(&callback)), None);
		Ok(())
	} else {
		Err(Error::new("Macrotask Queue has not been initialised.", None))
	}
}

const FUNCTIONS: &[JSFunctionSpec] = &[
	function_spec!(set_timeout, c"setTimeout", 1),
	function_spec!(set_interval, c"setInterval", 1),
	function_spec!(clear_timeout, c"clearTimeout", 0),
	function_spec!(clear_interval, c"clearInterval", 0),
	function_spec!(queue_macrotask, c"queueMacrotask", 1),
	JSFunctionSpec::ZERO,
];

pub fn define(cx: &Context, global: &Object) -> bool {
	unsafe { global.define_methods(cx, FUNCTIONS) }
}
