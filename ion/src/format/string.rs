/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::fmt;
use std::fmt::{Display, Formatter};

use colored::Colorize as _;
use encoding_rs::UTF_8;
use mozjs::jsval::StringValue;

use crate::format::Config;
use crate::{Context, Local, Value};

pub fn format_string<'cx>(cx: &'cx Context, cfg: Config, string: &'cx crate::String<'cx>) -> StringDisplay<'cx> {
	StringDisplay { cx, string, cfg }
}

#[must_use]
pub struct StringDisplay<'cx> {
	cx: &'cx Context,
	string: &'cx crate::String<'cx>,
	cfg: Config,
}

impl Display for StringDisplay<'_> {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		let colour = self.cfg.colours.string;
		if self.cfg.quoted {
			if let Ok(str) = self.string.to_owned(self.cx) {
				write!(f, "{0}{1}{0}", r#"""#.color(colour), str.color(colour))
			} else {
				let value = StringValue(unsafe { &*self.string.get() });
				let value = Value::from(unsafe { Local::from_marked(&value) });
				let str = value.to_source(self.cx).to_owned(self.cx).unwrap();
				str.color(colour).fmt(f)
			}
		} else if let Ok(str) = self.string.to_owned(self.cx) {
			str.fmt(f)
		} else {
			let input = self.string.as_wtf16(self.cx).unwrap();
			let mut encoder = UTF_8.new_encoder();
			let buf_len = encoder.max_buffer_length_from_utf16_if_no_unmappables(input.len()).unwrap();
			let mut buf = Vec::with_capacity(buf_len);
			let (_, _, _, _) = encoder.encode_from_utf16(input, &mut buf, true);
			let str = unsafe { String::from_utf8_unchecked(buf) };
			str.fmt(f)
		}
	}
}
