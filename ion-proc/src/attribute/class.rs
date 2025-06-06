/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use syn::meta::ParseNestedMeta;
use syn::{LitStr, Result};

use crate::attribute::name::Name;
use crate::attribute::{ArgumentError, ParseArgument as _, ParseArgumentWith as _, ParseAttribute};
use crate::class::method::MethodKind;

// TODO: Add `inspectable` to provide `toString` and `toJSON`
#[derive(Default)]
pub(crate) struct ClassAttribute {
	pub(crate) name: Option<LitStr>,
}

impl ParseAttribute for ClassAttribute {
	fn parse(&mut self, meta: &ParseNestedMeta) -> Result<()> {
		self.name.parse_argument(meta, "name", "Class")?;
		Ok(())
	}
}

#[derive(Default)]
pub(crate) struct MethodAttribute {
	pub(crate) name: Option<Name>,
	pub(crate) aliases: Vec<LitStr>,
	pub(crate) kind: Option<MethodKind>,
	pub(crate) skip: bool,
}

impl ParseAttribute for MethodAttribute {
	fn parse(&mut self, meta: &ParseNestedMeta) -> Result<()> {
		const METHOD_KIND_ERROR: ArgumentError =
			ArgumentError::Full("Method cannot have multiple `constructor`, `get`, or `set` attributes.");

		self.name.parse_argument(meta, "name", "Method")?;
		self.aliases.parse_argument(meta, "alias", None)?;
		self.kind
			.parse_argument_with(meta, MethodKind::Constructor, "constructor", METHOD_KIND_ERROR)?;
		self.kind.parse_argument_with(meta, MethodKind::Getter, "get", METHOD_KIND_ERROR)?;
		self.kind.parse_argument_with(meta, MethodKind::Setter, "set", METHOD_KIND_ERROR)?;
		self.skip.parse_argument(meta, "skip", "Method")?;

		Ok(())
	}
}
