/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use ion::{ClassDefinition as _, Context, Iterator, Object};

pub mod abort;
pub mod base64;
pub mod clone;
pub mod console;
pub mod encoding;
#[cfg(feature = "fetch")]
pub mod fetch;
pub mod file;
pub mod microtasks;
pub mod streams;
pub mod timers;
pub mod url;

pub fn init_globals(cx: &Context, global: &Object) -> bool {
	let result = base64::define(cx, global)
		&& clone::define(cx, global)
		&& console::define(cx, global)
		&& encoding::define(cx, global)
		&& file::define(cx, global)
		&& streams::define(cx, global)
		&& url::define(cx, global)
		&& Iterator::init_class(cx, global).0;

	#[cfg(feature = "fetch")]
	{
		result && fetch::define(cx, global)
	}
	#[cfg(not(feature = "fetch"))]
	{
		result
	}
}

pub fn init_timers(cx: &Context, global: &Object) -> bool {
	timers::define(cx, global) && abort::define(cx, global)
}

pub fn init_microtasks(cx: &Context, global: &Object) -> bool {
	microtasks::define(cx, global)
}
