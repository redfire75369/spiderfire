/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::borrow::Cow;
use std::fmt::{Display, Formatter};
use std::{fmt, str};

pub use config::Config;

use crate::format::object::format_object;
use crate::format::primitive::format_primitive;
use crate::{Context, Value};

pub mod array;
pub mod boxed;
mod config;
pub mod date;
pub mod descriptor;
pub mod function;
pub mod key;
pub mod object;
mod prefix;
pub mod primitive;
pub mod promise;
pub mod regexp;
mod string;
pub mod symbol;
pub mod typedarray;

pub const INDENT: &str = "  ";
pub const NEWLINE: &str = "\n";

#[must_use]
pub fn indent_str(indentation: usize) -> Cow<'static, str> {
	const MAX_INDENTS: usize = 128;
	static INDENTS: &str = match str::from_utf8(&[b' '; MAX_INDENTS * 2]) {
		Ok(indents) => indents,
		_ => unreachable!(),
	};

	if indentation <= 128 {
		Cow::Borrowed(&INDENTS[0..indentation * INDENT.len()])
	} else {
		Cow::Owned(INDENT.repeat(indentation))
	}
}

/// Formats a [JavaScript Value](Value) with the given [configuration](Config).
pub fn format_value<'cx>(cx: &'cx Context, cfg: Config, value: &'cx Value<'cx>) -> ValueDisplay<'cx> {
	ValueDisplay { cx, value, cfg }
}

#[must_use]
pub struct ValueDisplay<'cx> {
	cx: &'cx Context,
	value: &'cx Value<'cx>,
	cfg: Config,
}

impl Display for ValueDisplay<'_> {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		if self.value.handle().is_object() {
			format_object(self.cx, self.cfg, self.value.to_object(self.cx)).fmt(f)
		} else {
			format_primitive(self.cx, self.cfg, self.value).fmt(f)
		}
	}
}
