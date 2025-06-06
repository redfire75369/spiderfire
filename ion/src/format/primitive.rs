/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::fmt;
use std::fmt::{Display, Formatter};

use colored::Colorize as _;
use itoa::Buffer;

use crate::bigint::BigInt;
use crate::conversions::FromValue as _;
use crate::format::Config;
use crate::format::string::format_string;
use crate::format::symbol::format_symbol;
use crate::{Context, Symbol, Value};

/// Formats a primitive value using the given [configuration](Config).
/// The supported types are `boolean`, `number`, `string`, `symbol`, `null` and `undefined`.
pub fn format_primitive<'cx>(cx: &'cx Context, cfg: Config, value: &'cx Value<'cx>) -> PrimitiveDisplay<'cx> {
	PrimitiveDisplay { cx, value, cfg }
}

#[must_use]
pub struct PrimitiveDisplay<'cx> {
	cx: &'cx Context,
	value: &'cx Value<'cx>,
	cfg: Config,
}

impl Display for PrimitiveDisplay<'_> {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		let colours = self.cfg.colours;

		let value = self.value.handle();
		if value.is_boolean() {
			value.to_boolean().to_string().color(colours.boolean).fmt(f)
		} else if value.is_int32() {
			let int = value.to_int32();
			let mut buffer = Buffer::new();
			buffer.format(int).color(colours.number).fmt(f)
		} else if value.is_double() {
			let number = value.to_double();

			if number == f64::INFINITY {
				"Infinity".color(colours.number).fmt(f)
			} else if number == f64::NEG_INFINITY {
				"-Infinity".color(colours.number).fmt(f)
			} else {
				number.to_string().color(colours.number).fmt(f)
			}
		} else if value.is_string() {
			let str = crate::String::from_value(self.cx, self.value, true, ()).unwrap();
			format_string(self.cx, self.cfg, &str).fmt(f)
		} else if value.is_null() {
			"null".color(colours.null).fmt(f)
		} else if value.is_undefined() {
			"undefined".color(colours.undefined).fmt(f)
		} else if value.is_bigint() {
			let bi = BigInt::from(self.cx.root(value.to_bigint()));
			bi.to_string(self.cx, 10).unwrap().to_owned(self.cx)?.color(colours.bigint).fmt(f)?;
			"n".color(colours.bigint).fmt(f)
		} else if value.is_symbol() {
			let symbol = Symbol::from(self.cx.root(value.to_symbol()));
			format_symbol(self.cx, self.cfg, &symbol).fmt(f)
		} else if value.is_magic() {
			"<magic>".color(colours.boolean).fmt(f)
		} else {
			unreachable!("Expected Primitive")
		}
	}
}
