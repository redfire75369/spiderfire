/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::ffi::CString;

use convert_case::{Case, Casing as _};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};
use syn::{ItemFn, LitCStr, Result, Signature, Type};

use crate::attribute::name::Name;
use crate::function::parameter::Parameters;
use crate::function::wrapper::impl_wrapper_fn;
use crate::function::{check_abi, impl_fn_body, set_signature};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum MethodKind {
	Constructor,
	Getter,
	Setter,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum MethodReceiver {
	Dynamic,
	Static,
}

#[derive(Clone)]
pub(super) struct Method {
	pub(super) receiver: MethodReceiver,
	pub(super) method: ItemFn,
	pub(super) nargs: u16,
	pub(super) names: Vec<Name>,
}

impl Method {
	pub(super) fn to_specs(&self, ion: &TokenStream, class: &Ident) -> Vec<TokenStream> {
		let ident = &self.method.sig.ident;
		let nargs = self.nargs;

		self.names
			.iter()
			.map(|name| match name {
				Name::String(literal) => {
					let mut name = literal.value();
					if name.is_case(Case::Snake) {
						name = name.to_case(Case::Camel);
					}
					let literal = LitCStr::new(&CString::new(name).unwrap(), literal.span());
					quote!(#ion::function_spec!(#class::#ident, #literal, #nargs, #ion::flags::PropertyFlags::CONSTANT_ENUMERATED))
				}
				Name::Symbol(symbol) => {
					quote!(#ion::function_spec_symbol!(#class::#ident, #symbol, #nargs, #ion::flags::PropertyFlags::CONSTANT))
				}
			})
			.collect()
	}
}

pub(super) fn impl_method<F>(
	ion: &TokenStream, mut method: ItemFn, ty: &Type, class: &str, predicate: F,
) -> Result<(Method, Parameters)>
where
	F: FnOnce(&Signature) -> Result<()>,
{
	let fn_ident = &method.sig.ident;
	let (wrapper, parameters) = impl_wrapper_fn(ion, method.clone(), Some(ty), false, &format!("{class}.{fn_ident}"))?;

	predicate(&method.sig)?;

	check_abi(&mut method)?;
	set_signature(&mut method);

	method.attrs.clear();
	method.sig.ident = format_ident!("__ion_bindings_method_{}", method.sig.ident);
	method.block = impl_fn_body(ion, &wrapper)?;

	let method = Method {
		receiver: if parameters.this.is_some() {
			MethodReceiver::Dynamic
		} else {
			MethodReceiver::Static
		},
		method,
		nargs: parameters.nargs,
		names: vec![],
	};
	Ok((method, parameters))
}
