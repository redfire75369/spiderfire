/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use proc_macro2::{Ident, TokenStream};
use syn::meta::ParseNestedMeta;
use syn::{Attribute, Result, parse_quote};

use crate::attribute::{ParseArgument as _, ParseAttribute};

#[derive(Default)]
struct CrateAttribute {
	krate: Option<TokenStream>,
}

impl ParseAttribute for CrateAttribute {
	fn parse(&mut self, meta: &ParseNestedMeta) -> Result<()> {
		self.krate.parse_argument(meta, "crate", "Attribute")?;
		Ok(())
	}

	fn from_attributes_mut<I: ?Sized>(path: &I, attrs: &mut Vec<Attribute>) -> Result<Self>
	where
		Ident: PartialEq<I>,
	{
		let mut indices = Vec::new();
		let mut attribute = Self::default();
		for (i, attr) in attrs.iter().enumerate() {
			if attr.path().is_ident(path) {
				if attr.parse_nested_meta(|meta| attribute.parse(&meta)).is_ok() {
					indices.push(i);
				}
				break;
			}
		}
		while let Some(index) = indices.pop() {
			attrs.remove(index);
		}
		Ok(attribute)
	}
}

pub(crate) fn crate_from_attributes(attrs: &mut Vec<Attribute>) -> TokenStream {
	let attribute = CrateAttribute::from_attributes_mut("ion", attrs).unwrap();
	attribute.krate.unwrap_or_else(|| parse_quote!(::ion))
}
