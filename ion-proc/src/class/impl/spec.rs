/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::collections::HashMap;

use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote_spanned};
use syn::{ImplItemFn, Result, Type, parse_quote, parse2};

use crate::class::accessor::{Accessor, flatten_accessors};
use crate::class::method::Method;
use crate::class::property::Property;

#[derive(Default)]
pub(super) struct PrototypeSpecs {
	pub(super) methods: (Vec<Method>, Vec<Method>),
	pub(super) properties: (Vec<Property>, Vec<Property>),
	pub(super) accessors: (HashMap<String, Accessor>, HashMap<String, Accessor>),
}

impl PrototypeSpecs {
	pub(super) fn to_impl_fns(
		&self, ion: &TokenStream, span: Span, ident: &Ident,
	) -> Result<(Vec<ImplItemFn>, Vec<ImplItemFn>)> {
		let mut impl_fns = Vec::with_capacity(4);

		if !self.methods.0.is_empty() {
			impl_fns.push(methods_to_impl_fn(ion, span, ident, &self.methods.0, false)?);
		}
		if !self.methods.1.is_empty() {
			impl_fns.push(methods_to_impl_fn(ion, span, ident, &self.methods.1, true)?);
		}

		if !self.properties.0.is_empty() || !self.accessors.0.is_empty() {
			impl_fns.push(properties_to_spec_function(
				ion,
				span,
				ident,
				&self.properties.0,
				&self.accessors.0,
				false,
			)?);
		}
		if !self.properties.1.is_empty() || !self.accessors.1.is_empty() {
			impl_fns.push(properties_to_spec_function(
				ion,
				span,
				ident,
				&self.properties.1,
				&self.accessors.1,
				true,
			)?);
		}

		Ok(impl_fns.into_iter().unzip())
	}

	pub(super) fn into_functions(self) -> Vec<Method> {
		let len = self.methods.0.len() + self.methods.1.len() + self.accessors.0.len() + self.accessors.1.len();
		let mut functions = Vec::with_capacity(len);

		functions.extend(self.methods.0);
		functions.extend(self.methods.1);
		functions.extend(flatten_accessors(self.accessors.0));
		functions.extend(flatten_accessors(self.accessors.1));

		functions
	}
}

fn methods_to_impl_fn(
	ion: &TokenStream, span: Span, class: &Ident, methods: &[Method], r#static: bool,
) -> Result<(ImplItemFn, ImplItemFn)> {
	let mut ident = parse_quote!(functions);
	if r#static {
		ident = format_ident!("static_{}", ident);
	}
	let function_ident = format_ident!("__ion_{}_specs", ident);

	let specs: Vec<_> = methods.iter().flat_map(|method| method.to_specs(ion, class)).collect();

	let ty = parse_quote!(::mozjs::jsapi::JSFunctionSpec);
	Ok((
		spec_function(span, &function_ident, &specs, &ty)?,
		def_function(span, &ident, &function_ident, &ty)?,
	))
}

fn properties_to_spec_function(
	ion: &TokenStream, span: Span, class: &Ident, properties: &[Property], accessors: &HashMap<String, Accessor>,
	r#static: bool,
) -> Result<(ImplItemFn, ImplItemFn)> {
	let mut ident = parse_quote!(properties);
	if r#static {
		ident = format_ident!("static_{}", ident);
	}
	let function_ident = format_ident!("__ion_{}_specs", ident);

	let mut specs: Vec<_> = properties.iter().flat_map(|property| property.to_specs(ion, class)).collect();
	for accessor in accessors.values() {
		specs.extend(accessor.to_specs(ion, class));
	}

	let ty = parse_quote!(::mozjs::jsapi::JSPropertySpec);
	Ok((
		spec_function(span, &function_ident, &specs, &ty)?,
		def_function(span, &ident, &function_ident, &ty)?,
	))
}

fn spec_function(span: Span, function_ident: &Ident, specs: &[TokenStream], ty: &Type) -> Result<ImplItemFn> {
	assert!(!specs.is_empty());
	parse2(quote_spanned!(span => fn #function_ident() -> &'static [#ty] {
		static SPECS: &[#ty] = &[
			#(#specs,)*
			#ty::ZERO,
		];
		SPECS
	}))
}

fn def_function(span: Span, ident: &Ident, function_ident: &Ident, ty: &Type) -> Result<ImplItemFn> {
	parse2(
		quote_spanned!(span => fn #ident() -> ::std::option::Option<&'static [#ty]> {
			::std::option::Option::Some(Self::#function_ident())
		}),
	)
}
