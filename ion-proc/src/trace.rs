/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use proc_macro2::{Ident, Span};
use syn::spanned::Spanned;
use syn::{parse2, Arm, Block, Data, DeriveInput, Error, Fields, Generics, ItemImpl, Result};

use crate::attribute::trace::TraceAttribute;
use crate::attribute::ParseAttribute;
use crate::utils::{add_trait_bounds, wrap_in_fields_group};

pub(super) fn impl_trace(mut input: DeriveInput) -> Result<ItemImpl> {
	add_trait_bounds(&mut input.generics, &parse_quote!(::mozjs::gc::Traceable));
	let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
	let impl_generics: Generics = parse2(quote_spanned!(impl_generics.span() => #impl_generics))?;

	let name = &input.ident;
	let body = impl_body(input.span(), &input.data)?;

	parse2(quote_spanned!(input.span() =>
		#[automatically_derived]
		unsafe impl #impl_generics ::mozjs::gc::Traceable for #name #ty_generics #where_clause {
			unsafe fn trace(&self, __ion_tracer: *mut ::mozjs::jsapi::JSTracer) {
				#[allow(unused_unsafe)]
				unsafe #body
			}
		}
	))
}

fn impl_body(span: Span, data: &Data) -> Result<Box<Block>> {
	match data {
		Data::Struct(r#struct) => {
			let (idents, traced) = field_idents(&r#struct.fields)?;
			let wrapped = if matches!(r#struct.fields, Fields::Named(_) | Fields::Unnamed(_)) {
				wrap_in_fields_group(idents, &r#struct.fields)
			} else {
				quote!()
			};
			parse2(quote_spanned!(span => {
				let Self #wrapped = self;
				#(::mozjs::gc::Traceable::trace(#traced, __ion_tracer));*
			}))
		}
		Data::Enum(r#enum) => {
			let matches: Vec<Arm> = r#enum
				.variants
				.iter()
				.map(|variant| {
					let ident = &variant.ident;
					let (idents, traced) = field_idents(&variant.fields)?;
					let wrapped = if matches!(variant.fields, Fields::Named(_) | Fields::Unnamed(_)) {
						wrap_in_fields_group(idents, &variant.fields)
					} else {
						quote!()
					};

					parse2(quote_spanned!(variant.span() =>
						Self::#ident #wrapped => {
							#(::mozjs::gc::Traceable::trace(#traced, __ion_tracer));*
						}
					))
				})
				.collect::<Result<_>>()?;
			parse2(quote_spanned!(span => {
				match self {
					#(#matches)*
				}
			}))
		}
		Data::Union(_) => Err(Error::new(
			span,
			"#[derive(Traceable)] is not implemented for union types.",
		)),
	}
}

fn field_idents(fields: &Fields) -> Result<(Vec<Ident>, Vec<Ident>)> {
	let mut idents = Vec::with_capacity(fields.len());
	let mut traced = Vec::with_capacity(fields.len());
	for (index, field) in fields.iter().enumerate() {
		let attribute = TraceAttribute::from_attributes("trace", &field.attrs)?;
		let ident = match &field.ident {
			Some(ident) => ident.clone(),
			None => format_ident!("_self_{}", index),
		};
		idents.push(ident.clone());
		if !attribute.no_trace {
			traced.push(ident);
		}
	}
	Ok((idents, traced))
}
