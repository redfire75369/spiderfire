/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::fmt;
use std::fmt::{Display, Formatter, Write};

use colored::Colorize;
use mozjs::jsapi::JSProtoKey;

use crate::format::descriptor::format_descriptor;
use crate::format::object::write_remaining;
use crate::format::prefix::write_prefix;
use crate::format::{Config, NEWLINE, indent_str};
use crate::{Array, Context};

/// Formats an [JavaScript Array](Array) using the given [configuration](Config).
pub fn format_array<'cx>(cx: &'cx Context, cfg: Config, array: &'cx Array<'cx>) -> ArrayDisplay<'cx> {
	ArrayDisplay { cx, array, cfg }
}

#[must_use]
pub struct ArrayDisplay<'cx> {
	cx: &'cx Context,
	array: &'cx Array<'cx>,
	cfg: Config,
}

impl Display for ArrayDisplay<'_> {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		let colour = self.cfg.colours.array;

		write_prefix(
			f,
			self.cx,
			self.cfg,
			self.array.as_object(),
			"Array",
			JSProtoKey::JSProto_Array,
		)?;

		if self.cfg.depth > 4 {
			return "[Array]".color(colour).fmt(f);
		}

		let length = self.array.len(self.cx);
		if length == 0 {
			return "[]".color(colour).fmt(f);
		}

		"[".color(colour).fmt(f)?;
		let (remaining, inner) = if self.cfg.multiline {
			f.write_str(NEWLINE)?;
			let shown = length.clamp(0, 100);

			let inner = indent_str((self.cfg.indentation + self.cfg.depth + 1) as usize);

			for index in 0..shown {
				inner.fmt(f)?;
				let desc = self.array.get_descriptor(self.cx, index)?.unwrap();
				format_descriptor(self.cx, self.cfg, &desc, Some(self.array.as_object())).fmt(f)?;
				",".color(colour).fmt(f)?;
				f.write_str(NEWLINE)?;
			}

			(length - shown, Some(inner))
		} else {
			f.write_char(' ')?;
			let shown = length.clamp(0, 3);

			for index in 0..shown {
				let desc = self.array.get_descriptor(self.cx, index)?.unwrap();
				format_descriptor(self.cx, self.cfg, &desc, Some(self.array.as_object())).fmt(f)?;

				if index != shown - 1 {
					",".color(colour).fmt(f)?;
					f.write_char(' ')?;
				}
			}

			(length - shown, None)
		};

		write_remaining(f, remaining as usize, inner.as_deref(), colour)?;

		if self.cfg.multiline {
			indent_str((self.cfg.indentation + self.cfg.depth) as usize).fmt(f)?;
		}

		"]".color(colour).fmt(f)
	}
}
