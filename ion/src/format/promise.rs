/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::fmt;
use std::fmt::{Display, Formatter, Write as _};

use colored::Colorize as _;
use mozjs::jsapi::PromiseState;

use crate::format::{Config, format_value, indent_str};
use crate::{Context, Promise};

/// Formats a [Promise] with the given [configuration](Config).
/// ### Format
/// ```js
/// Promise { <#state> <#result> }
/// ```
pub fn format_promise<'cx>(cx: &'cx Context, cfg: Config, promise: &'cx Promise) -> PromiseDisplay<'cx> {
	PromiseDisplay { cx, promise, cfg }
}

#[must_use]
pub struct PromiseDisplay<'cx> {
	cx: &'cx Context,
	promise: &'cx Promise<'cx>,
	cfg: Config,
}

impl Display for PromiseDisplay<'_> {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		let colour = self.cfg.colours.promise;
		let state = self.promise.state();

		let state = match state {
			PromiseState::Pending => return "Promise { <pending> }".color(colour).fmt(f),
			PromiseState::Fulfilled => "<fulfilled>".color(colour),
			PromiseState::Rejected => "<rejected>".color(colour),
		};

		"Promise {".color(colour).fmt(f)?;
		let result = self.promise.result(self.cx);

		if self.cfg.multiline {
			let result = format_value(self.cx, self.cfg.depth(self.cfg.depth + 1), &result);

			f.write_char('\n')?;
			indent_str((self.cfg.indentation + self.cfg.depth + 1) as usize).fmt(f)?;
			state.fmt(f)?;
			f.write_char(' ')?;
			result.fmt(f)?;
			"\n}".color(colour).fmt(f)
		} else {
			let result = format_value(self.cx, self.cfg, &result);

			f.write_char(' ')?;
			state.fmt(f)?;
			f.write_char(' ')?;
			result.fmt(f)?;
			" }".color(colour).fmt(f)
		}
	}
}
