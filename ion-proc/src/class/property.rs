/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};
use syn::{ImplItemConst, Result, Type};

use crate::attribute::ParseAttribute as _;
use crate::attribute::name::Name;
use crate::attribute::property::PropertyAttribute;
use crate::utils::path_ends_with;

#[derive(Clone, Debug)]
pub(super) enum PropertyType {
	Int32,
	Double,
	String,
}

#[derive(Clone)]
pub(super) struct Property {
	pub(super) ty: PropertyType,
	pub(super) ident: Ident,
	pub(super) names: Vec<Name>,
}

impl Property {
	pub(super) fn from_const(r#const: &mut ImplItemConst) -> Result<Option<(Property, bool)>> {
		let mut names = Vec::new();

		let attribute = PropertyAttribute::from_attributes("ion", &mut r#const.attrs, ())?;

		let PropertyAttribute { name, alias, skip, r#static } = attribute;
		for alias in alias {
			names.push(Name::String(alias));
		}

		r#const.attrs.clear();
		if skip {
			return Ok(None);
		}

		let ident = r#const.ident.clone();

		match name {
			Some(name) => names.insert(0, name),
			None => names.insert(0, Name::from_string(ident.to_string(), ident.span())),
		}

		match &r#const.ty {
			Type::Path(ty) => {
				if path_ends_with(&ty.path, "i32") {
					Ok(Some((Property { ty: PropertyType::Int32, ident, names }, r#static)))
				} else if path_ends_with(&ty.path, "f64") {
					Ok(Some((Property { ty: PropertyType::Double, ident, names }, r#static)))
				} else {
					Ok(None)
				}
			}
			Type::Reference(re) => {
				if let Type::Path(ty) = &*re.elem
					&& path_ends_with(&ty.path, "str")
				{
					return Ok(Some((Property { ty: PropertyType::String, ident, names }, r#static)));
				}
				Ok(None)
			}
			_ => Ok(None),
		}
	}

	pub(super) fn to_specs(&self, ion: &TokenStream, class: &Ident) -> Vec<TokenStream> {
		let ident = &self.ident;

		self.names
			.iter()
			.map(|name| {
				let mut function_ident = format_ident!("create_property_spec");

				let (key, flags) = name.to_property_spec(ion, &mut function_ident);

				function_ident = match self.ty {
					PropertyType::Int32 => format_ident!("{}_int", function_ident),
					PropertyType::Double => format_ident!("{}_double", function_ident),
					PropertyType::String => format_ident!("{}_string", function_ident),
				};

				quote!(#ion::spec::#function_ident(#key, #class::#ident, #flags))
			})
			.collect()
	}
}
