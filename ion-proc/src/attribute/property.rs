/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use syn::meta::ParseNestedMeta;
use syn::{LitStr, Result};

use crate::attribute::name::Name;
use crate::attribute::{ParseArgument as _, ParseAttribute};

#[derive(Default)]
pub(crate) struct PropertyAttribute {
	pub(crate) name: Option<Name>,
	pub(crate) alias: Vec<LitStr>,
	pub(crate) skip: bool,
	pub(crate) r#static: bool,
}

impl ParseAttribute for PropertyAttribute {
	type Parent<'a> = ();

	fn parse(&mut self, meta: &ParseNestedMeta) -> Result<bool> {
		let success = self.name.parse_argument(meta, "name", "Property")?
			|| self.alias.parse_argument(meta, "alias", None)?
			|| self.skip.parse_argument(meta, "skip", "Property")?
			|| self.r#static.parse_argument(meta, "static", "Property")?;

		Ok(success)
	}
}
