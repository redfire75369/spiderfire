/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use syn::meta::ParseNestedMeta;
use syn::{Expr, Result};

use crate::attribute::{ParseArgument as _, ParseAttribute};

#[derive(Default)]
pub(crate) struct ParameterAttribute {
	pub(crate) this: bool,
	pub(crate) convert: Option<Box<Expr>>,
}

impl ParseAttribute for ParameterAttribute {
	type Parent<'a> = ();

	fn parse(&mut self, meta: &ParseNestedMeta) -> Result<bool> {
		let success = self.this.parse_argument(meta, "this", "Parameter")?
			|| self.convert.parse_argument(meta, "convert", "Parameter")?;

		if self.this && self.convert.is_some() {
			return Err(meta.error("Parameter with `this` attribute cannot have `convert` attributes."));
		}

		Ok(success)
	}
}
