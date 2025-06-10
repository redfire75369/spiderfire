/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use proc_macro2::{Ident, Span};
use quote::{format_ident, quote, quote_spanned};
use syn::spanned::Spanned as _;
use syn::{Block, Data, DeriveInput, Error, Fields, Generics, ItemImpl, Result, parse_quote, parse2};

use crate::attribute::ParseAttribute as _;
use crate::attribute::trace::TraceAttribute;
use crate::utils::{add_trait_bounds, wrap_in_fields_group};

pub(super) fn impl_trace(mut input: DeriveInput) -> Result<ItemImpl> {
	add_trait_bounds(&mut input.generics, &parse_quote!(::mozjs::gc::Traceable));
	let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
	let impl_generics: Generics = parse2(quote_spanned!(impl_generics.span() => #impl_generics))?;

	let name = &input.ident;
	let body = impl_body(input.span(), &mut input.data)?;

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

fn impl_body(span: Span, data: &mut Data) -> Result<Box<Block>> {
	let mut variants = Vec::new();
	let mut variant_idents = Vec::new();
	let mut variant_traced = Vec::new();
	let mut fields = Vec::new();

	match data {
		Data::Struct(r#struct) => {
			let (idents, traced) = field_idents(&mut r#struct.fields)?;
			variants.push(quote!(Self));
			variant_idents.push(idents);
			variant_traced.push(traced);
			fields.push(r#struct.fields.clone());
		}
		Data::Enum(r#enum) => {
			for variant in &mut r#enum.variants {
				let ident = &variant.ident;
				let (idents, traced) = field_idents(&mut variant.fields)?;
				variants.push(quote!(Self::#ident));
				variant_idents.push(idents);
				variant_traced.push(traced);
				fields.push(variant.fields.clone());
			}
		}
		Data::Union(_) => {
			return Err(Error::new(
				span,
				"#[derive(Traceable)] is not implemented for union types.",
			));
		}
	}

	let wrapped = variant_idents.into_iter().enumerate().map(|(index, idents)| {
		if matches!(&fields[index], Fields::Named(_) | Fields::Unnamed(_)) {
			wrap_in_fields_group(idents, &fields[index])
		} else {
			quote!()
		}
	});

	parse2(quote_spanned!(span => {
		match self {
			#(#variants #wrapped => {
				#(::mozjs::gc::Traceable::trace(#variant_traced, __ion_tracer));*
			},)*
		}
	}))
}

fn field_idents(fields: &mut Fields) -> Result<(Vec<Ident>, Vec<Ident>)> {
	let mut idents = Vec::with_capacity(fields.len());
	let mut traced = Vec::with_capacity(fields.len());
	for (index, field) in fields.iter_mut().enumerate() {
		let attribute = TraceAttribute::from_attributes("trace", &mut field.attrs, ())?;
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
