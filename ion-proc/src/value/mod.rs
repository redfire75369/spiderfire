/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use convert_case::{Case, Casing};
pub(crate) use from::*;
use proc_macro2::Ident;
use quote::format_ident;
use syn::Field;
pub(crate) use to::*;

pub(crate) mod from;
pub(crate) mod to;

fn field_to_ident_key(field: &Field, index: usize) -> (Ident, String) {
	if let Some(ident) = &field.ident {
		(ident.clone(), ident.to_string().to_case(Case::Camel))
	} else {
		let ident = format_ident!("_self_{}", index);
		(ident, index.to_string())
	}
}
