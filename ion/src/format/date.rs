/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::fmt;
use std::fmt::{Display, Formatter};

use colored::Colorize as _;

use crate::format::Config;
use crate::{Context, Date};

/// Formats a [JavaScript Date](Date) using the given [configuration](Config).
pub fn format_date<'cx>(cx: &'cx Context, cfg: Config, date: &'cx Date<'cx>) -> DateDisplay<'cx> {
	DateDisplay { cx, date, cfg }
}

#[must_use]
pub struct DateDisplay<'cx> {
	cx: &'cx Context,
	date: &'cx Date<'cx>,
	cfg: Config,
}

impl Display for DateDisplay<'_> {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		if let Some(date) = self.date.to_date(self.cx) {
			date.to_string().color(self.cfg.colours.date).fmt(f)
		} else {
			panic!("Failed to unbox Date");
		}
	}
}
