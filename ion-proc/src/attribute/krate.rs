/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use proc_macro2::TokenStream;
use syn::meta::ParseNestedMeta;
use syn::{Attribute, Result, parse_quote};

use crate::attribute::{ParseArgument as _, ParseAttribute};

#[derive(Default)]
struct CrateAttribute {
	krate: Option<TokenStream>,
}

impl ParseAttribute for CrateAttribute {
	type Parent<'a> = ();

	fn parse(&mut self, meta: &ParseNestedMeta) -> Result<bool> {
		self.krate.parse_argument(meta, "crate", "Attribute")
	}
}

pub(crate) fn crate_from_attributes(attrs: &mut Vec<Attribute>) -> TokenStream {
	let attribute = CrateAttribute::from_attributes("ion", attrs, ()).unwrap();
	attribute.krate.unwrap_or_else(|| parse_quote!(::ion))
}
