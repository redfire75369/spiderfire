/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::fmt;
use std::fmt::{Display, Formatter, Write as _};

use colored::Colorize as _;

use crate::format::object::format_object;
use crate::format::primitive::format_primitive;
use crate::format::{Config, format_value};
use crate::{Context, Function, Object, PropertyDescriptor};

/// Formats a [descriptor](PropertyDescriptor) with the given [configuration](Config).
pub fn format_descriptor<'cx>(
	cx: &'cx Context, cfg: Config, desc: &'cx PropertyDescriptor<'cx>, object: Option<&'cx Object<'cx>>,
) -> DescriptorDisplay<'cx> {
	DescriptorDisplay { cx, cfg, desc, object }
}

#[must_use]
pub struct DescriptorDisplay<'cx> {
	cx: &'cx Context,
	cfg: Config,
	desc: &'cx PropertyDescriptor<'cx>,
	object: Option<&'cx Object<'cx>>,
}

impl Display for DescriptorDisplay<'_> {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		let color = self.cfg.colours.function;

		if let Some(getter) = self.desc.getter(self.cx) {
			"[Getter".color(color).fmt(f)?;
			if self.desc.setter(self.cx).is_some() {
				"/Setter".color(color).fmt(f)?;
			}

			if let Some(object) = self.object {
				format_getter(f, self.cx, &getter, object, self.cfg)
			} else {
				"]".color(color).fmt(f)
			}
		} else if self.desc.setter(self.cx).is_some() {
			"[Setter]".color(color).fmt(f)
		} else {
			match self.desc.value(self.cx) {
				Some(value) => format_value(self.cx, self.cfg.depth(self.cfg.depth + 1).quoted(true), &value).fmt(f),
				None => f.write_str("<empty descriptor>"),
			}
		}
	}
}

fn format_getter(f: &mut Formatter<'_>, cx: &Context, getter: &Function, object: &Object, cfg: Config) -> fmt::Result {
	let color = cfg.colours.function;

	let value = match getter.call(cx, object, &[]) {
		Ok(value) => value,
		Err(report) => {
			f.write_str(" <Inspection threw ")?;
			match report {
				Some(mut report) => {
					report.stack = None;
					f.write_char('(')?;
					report.format(cx).fmt(f)?;
					f.write_char(')')?;
				}
				None => f.write_str("unknown error")?,
			}
			f.write_char('>')?;
			return "]".color(color).fmt(f);
		}
	};

	if value.handle().is_object() {
		"] ".color(color).fmt(f)?;
		format_object(cx, cfg.depth(cfg.depth + 1).quoted(true), value.to_object(cx)).fmt(f)
	} else {
		": ".color(color).fmt(f)?;
		format_primitive(cx, cfg.quoted(true), &value).fmt(f)?;
		"]".color(color).fmt(f)
	}
}
