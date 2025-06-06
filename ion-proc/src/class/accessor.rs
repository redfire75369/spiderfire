/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use convert_case::{Case, Casing as _};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};
use syn::spanned::Spanned as _;
use syn::{Error, ItemFn, Result, Type};

use crate::class::method::{Method, impl_method};
use crate::function::parameter::Parameters;

pub(super) struct Accessor(pub(super) Option<Method>, Option<Method>);

impl Accessor {
	pub(super) fn to_specs(&self, ion: &TokenStream, class: &Ident) -> Vec<TokenStream> {
		let names = self.0.as_ref().or(self.1.as_ref()).map(|method| &*method.names).unwrap_or_default();
		names
			.iter()
			.map(|name| {
				let mut function_ident = format_ident!("property_spec");

				let (key, flags) = name.to_property_spec(ion, &mut function_ident);

				match self {
					Accessor(Some(getter), Some(setter)) => {
						let getter = getter.method.sig.ident.clone();
						let setter = setter.method.sig.ident.clone();

						function_ident = format_ident!("{}_getter_setter", function_ident);
						quote!(#ion::#function_ident!(#class::#getter, #class::#setter, #key, #flags))
					}
					Accessor(Some(getter), None) => {
						let getter = getter.method.sig.ident.clone();

						function_ident = format_ident!("{}_getter", function_ident);
						quote!(#ion::#function_ident!(#class::#getter, #key, #flags))
					}
					Accessor(None, Some(setter)) => {
						let setter = setter.method.sig.ident.clone();
						function_ident = format_ident!("{}_getter", function_ident);
						quote!(#ion::#function_ident!(#class::#setter, #key, #flags))
					}
					Accessor(None, None) => {
						function_ident = format_ident!("create_{}_accessor", function_ident);
						quote!(
							#ion::spec::#function_ident(
								#key,
								::mozjs::jsapi::JSNativeWrapper { op: None, info: ::std::ptr::null_mut() },
								::mozjs::jsapi::JSNativeWrapper { op: None, info: ::std::ptr::null_mut() },
								#flags,
							)
						)
					}
				}
			})
			.collect()
	}
}

pub(super) fn get_accessor_name(mut name: String, is_setter: bool) -> String {
	let pat_snake = if is_setter { "set_" } else { "get_" };
	let pat_camel = if is_setter { "set" } else { "get" };
	if name.starts_with(pat_snake) {
		name.drain(0..4);
		if name.is_case(Case::Snake) {
			name = name.to_case(Case::Camel);
		}
	} else if name.starts_with(pat_camel) {
		name.drain(0..3);
		if name.is_case(Case::Pascal) {
			name = name.to_case(Case::Camel);
		}
	}
	name
}

pub(super) fn impl_accessor(
	ion: &TokenStream, method: ItemFn, ty: &Type, is_setter: bool, class: &str,
) -> Result<(Method, Parameters)> {
	let expected_args = i32::from(is_setter);
	let error_message = if is_setter {
		format!("Expected Setter to have {expected_args} argument")
	} else {
		format!("Expected Getter to have {expected_args} arguments")
	};
	let error = Error::new(method.sig.span(), error_message);

	let ident = if is_setter {
		format_ident!("__ion_bindings_setter_{}", method.sig.ident)
	} else {
		format_ident!("__ion_bindings_getter_{}", method.sig.ident)
	};
	let (mut accessor, parameters) = impl_method(ion, method, ty, class, |sig| {
		let parameters = Parameters::parse(&sig.inputs, Some(ty))?;
		let nargs: i32 = parameters
			.parameters
			.iter()
			.map(|param| i32::from(matches!(&*param.pat_ty.ty, Type::Path(_))))
			.sum();
		(nargs == expected_args).then_some(()).ok_or(error)
	})?;
	accessor.method.sig.ident = ident;

	Ok((accessor, parameters))
}

pub(super) fn insert_accessor(
	accessors: &mut HashMap<String, Accessor>, name: String, getter: Option<Method>, setter: Option<Method>,
) {
	match accessors.entry(name) {
		Entry::Occupied(mut o) => match (getter, setter) {
			(Some(g), Some(s)) => *o.get_mut() = Accessor(Some(g), Some(s)),
			(Some(g), None) => o.get_mut().0 = Some(g),
			(None, Some(s)) => o.get_mut().1 = Some(s),
			(None, None) => {}
		},
		Entry::Vacant(v) => {
			v.insert(Accessor(getter, setter));
		}
	}
}

pub(super) fn flatten_accessors(accessors: HashMap<String, Accessor>) -> impl Iterator<Item = Method> {
	accessors.into_iter().flat_map(|(_, Accessor(getter, setter))| [getter, setter]).flatten()
}
