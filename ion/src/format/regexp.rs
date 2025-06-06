/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::fmt;
use std::fmt::{Display, Formatter};

use colored::Colorize as _;

use crate::Context;
use crate::format::Config;
use crate::object::RegExp;

/// Formats a [RegExp object](RegExp) using the given [configuration](Config).
pub fn format_regexp<'cx>(cx: &'cx Context, cfg: Config, regexp: &'cx RegExp<'cx>) -> RegExpDisplay<'cx> {
	RegExpDisplay { cx, regexp, cfg }
}

#[must_use]
pub struct RegExpDisplay<'cx> {
	cx: &'cx Context,
	regexp: &'cx RegExp<'cx>,
	cfg: Config,
}

impl Display for RegExpDisplay<'_> {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		self.regexp.to_string(self.cx)?.color(self.cfg.colours.regexp).fmt(f)
	}
}
