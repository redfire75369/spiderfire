/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::fmt;
use std::fmt::{Display, Formatter};

use colored::Colorize as _;
use itoa::Buffer;

use crate::format::Config;
use crate::format::symbol::format_symbol;
use crate::{Context, OwnedKey};

/// Formats the [key of an object](OwnedKey) with the given [configuration](Config).
pub fn format_key<'cx>(cx: &'cx Context, cfg: Config, key: &'cx OwnedKey<'cx>) -> KeyDisplay<'cx> {
	KeyDisplay { cx, cfg, key }
}

#[must_use]
pub struct KeyDisplay<'cx> {
	cx: &'cx Context,
	cfg: Config,
	key: &'cx OwnedKey<'cx>,
}

impl Display for KeyDisplay<'_> {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		let colours = self.cfg.colours;
		match self.key {
			OwnedKey::Int(i) => {
				let mut buffer = Buffer::new();
				buffer.format(*i).color(colours.number).fmt(f)
			}
			OwnedKey::String(str) => write!(f, "{0}{1}{0}", r#"""#.color(colours.string), str.color(colours.string)),
			OwnedKey::Symbol(sym) => {
				"[".color(colours.symbol).fmt(f)?;
				format_symbol(self.cx, self.cfg, sym).fmt(f)?;
				"]".color(colours.symbol).fmt(f)
			}
			OwnedKey::Void => unreachable!("Property key <void> cannot be formatted."),
		}
	}
}
