/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use prettyplease::unparse;
use proc_macro2::{Ident, TokenStream};
use quote::quote;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned as _;
use syn::{
	Attribute, Error, Fields, GenericParam, Generics, Lifetime, LifetimeParam, Meta, Pat, Path, Result, Token, Type,
	TypeParamBound, parse_quote, parse2,
};

pub(crate) fn path_ends_with<I: ?Sized>(path: &Path, ident: &I) -> bool
where
	Ident: PartialEq<I>,
{
	if let Some(last) = path.segments.last() {
		&last.ident == ident
	} else {
		false
	}
}

pub(crate) fn pat_is_ident<I: ?Sized>(pat: &Pat, ident: &I) -> bool
where
	Ident: PartialEq<I>,
{
	if let Pat::Ident(pat) = pat {
		&pat.ident == ident
	} else {
		false
	}
}

pub(crate) fn add_trait_bounds(generics: &mut Generics, bound: &TypeParamBound) {
	for param in &mut generics.params {
		if let GenericParam::Type(type_param) = param {
			type_param.bounds.push(bound.clone());
		}
	}
}

pub(crate) fn add_lifetime_generic(impl_generics: &mut Generics, lifetime: Lifetime) {
	let has_cx = impl_generics.params.iter().any(|param| {
		if let GenericParam::Lifetime(lt) = param {
			lt.lifetime == lifetime
		} else {
			false
		}
	});

	if !has_cx {
		let param = GenericParam::Lifetime(LifetimeParam {
			attrs: Vec::new(),
			lifetime,
			colon_token: None,
			bounds: Punctuated::new(),
		});

		impl_generics.params.push(param);
	}
}

pub(crate) fn find_repr(attrs: &[Attribute]) -> Result<Option<Ident>> {
	let mut repr = None;
	for attr in attrs {
		if attr.path().is_ident("repr") {
			let nested = attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
			let allowed_reprs: [Ident; 8] = [
				parse_quote!(i8),
				parse_quote!(i16),
				parse_quote!(i32),
				parse_quote!(i64),
				parse_quote!(u8),
				parse_quote!(u16),
				parse_quote!(u32),
				parse_quote!(u64),
			];

			for meta in nested {
				if let Meta::Path(path) = &meta {
					for allowed_repr in &allowed_reprs {
						if path.is_ident(allowed_repr) {
							if repr.is_none() {
								repr = Some(path.get_ident().unwrap().clone());
							} else {
								return Err(Error::new(meta.span(), "Only One Representation Allowed in #[repr]"));
							}
						}
					}
				}
			}
		}
	}

	Ok(repr)
}

pub(crate) fn wrap_in_fields_group<I>(idents: I, fields: &Fields) -> TokenStream
where
	I: IntoIterator<Item = Ident>,
{
	let idents = idents.into_iter();
	match fields {
		Fields::Named(_) => quote!({ #(#idents,)* }),
		Fields::Unnamed(_) => quote!((#(#idents,)*)),
		Fields::Unit => quote!(),
	}
}

pub(crate) fn format_type(ty: &Type) -> String {
	let ty = unparse(
		&parse2(quote!(
			impl #ty {}
		))
		.unwrap(),
	);
	let mut ty = String::from(ty.trim());
	ty.drain((ty.len() - 2)..ty.len());
	ty.drain(0..4);
	String::from(ty.trim())
}

macro_rules! new_token {
    [$i:tt] => {
		<::syn::Token![$i]>::default()
    };
}

pub(crate) use new_token;
