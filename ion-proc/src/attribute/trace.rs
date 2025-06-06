/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use syn::meta::ParseNestedMeta;
use syn::parse::Result;

use crate::attribute::{ParseArgument as _, ParseAttribute};

#[derive(Debug, Default)]
pub(crate) struct TraceAttribute {
	pub(crate) no_trace: bool,
}

impl ParseAttribute for TraceAttribute {
	fn parse(&mut self, meta: &ParseNestedMeta) -> Result<()> {
		self.no_trace.parse_argument(meta, "no_trace", "Field")?;
		Ok(())
	}
}
