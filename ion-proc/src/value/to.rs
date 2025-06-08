/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::spanned::Spanned as _;
use syn::{Block, Data, DeriveInput, Error, Fields, Generics, ItemImpl, Result, parse_quote, parse2};

use crate::attribute::krate::crate_from_attributes;
use crate::attribute::value::{DataAttribute, FieldAttribute, Tag, VariantAttribute};
use crate::attribute::{Optional, ParseAttribute as _};
use crate::utils::{add_lifetime_generic, add_trait_bounds, wrap_in_fields_group};
use crate::value::field_to_ident_key;

pub(crate) fn impl_to_value(mut input: DeriveInput) -> Result<ItemImpl> {
	let ion = &crate_from_attributes(&mut input.attrs);

	add_trait_bounds(&mut input.generics, &parse_quote!(#ion::conversions::ToValue));
	let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
	let mut impl_generics: Generics = parse2(quote_spanned!(impl_generics.span() => #impl_generics))?;
	add_lifetime_generic(&mut impl_generics, parse_quote!('cx));

	let attribute = DataAttribute::from_attributes("ion", &input.attrs)?;

	let name = &input.ident;
	let (body, requires_object) = impl_body(ion, input.span(), &input.data, &attribute.tag, attribute.inherit)?;

	let prefix = requires_object.then(|| quote!(let __ion_object = #ion::Object::new(__ion_cx);));
	let postfix = requires_object.then(|| {
		quote!(
			#ion::conversions::ToValue::to_value(&__ion_object, __ion_cx, __ion_value);
		)
	});

	parse2(quote_spanned!(input.span() =>
		#[automatically_derived]
		impl #impl_generics #ion::conversions::ToValue<'cx> for #name #ty_generics #where_clause {
			fn to_value(&self, __ion_cx: &'cx #ion::Context, __ion_value: &mut #ion::Value) {
				#prefix
				#body
				#postfix
			}
		}
	))
}

fn impl_body(
	ion: &TokenStream, span: Span, data: &Data, tag: &Optional<Tag>, inherit: bool,
) -> Result<(TokenStream, bool)> {
	let (variants, idents, blocks, requires_object) = match data {
		Data::Struct(r#struct) => {
			let (idents, statements, requires_object) = map_fields(ion, &r#struct.fields, inherit)?;
			let wrapped = wrap_in_fields_group(idents, &r#struct.fields);
			let requirement = tag_requirement(ion, tag.0.as_ref(), None, requires_object)?;

			let block: Block = parse2(quote_spanned!(span => {
				#requirement
				#(#statements)*
			}))?;
			(None, vec![wrapped], vec![block], requires_object)
		}
		Data::Enum(r#enum) => {
			let mut variants = Vec::with_capacity(r#enum.variants.len());
			let mut variant_idents = Vec::with_capacity(r#enum.variants.len());
			let mut blocks = Vec::with_capacity(r#enum.variants.len());
			let mut requires_object = false;

			for variant in &r#enum.variants {
				let variant_string = variant.ident.to_string();

				let mut attribute = VariantAttribute::from_attributes("ion", &variant.attrs)?;
				attribute.merge(tag.0.as_ref(), inherit);
				if attribute.skip {
					continue;
				}

				let (idents, statements, req_object) = map_fields(ion, &variant.fields, attribute.inherit)?;
				let wrapped = wrap_in_fields_group(idents, &variant.fields);
				let requirement = tag_requirement(ion, attribute.tag.0.as_ref(), Some(variant_string), req_object)?;
				requires_object =
					requires_object || req_object || matches!(&tag.0, Some(Tag::External | Tag::Internal(_)));

				let block = parse2(quote_spanned!(span => {
					#requirement
					#(#statements)*
				}))?;

				variants.push(variant.ident.clone());
				variant_idents.push(wrapped);
				blocks.push(block);
			}

			(Some(variants), variant_idents, blocks, requires_object)
		}
		Data::Union(_) => {
			return Err(Error::new(
				span,
				"#[derive(ToValue)] is not implemented for union types",
			));
		}
	};

	let body = if let Some(variants) = variants {
		quote_spanned!(span => match self {
			#(Self::#variants #idents => #blocks)*
		})
	} else {
		quote_spanned!(span => match self {
			#(Self #idents => #blocks)*
		})
	};

	Ok((body, requires_object))
}

fn tag_requirement(
	ion: &TokenStream, tag: Option<&Tag>, variant: Option<String>, requires_object: bool,
) -> Result<Option<TokenStream>> {
	let Some(variant) = variant else {
		return if matches!(&tag, Some(Tag::External | Tag::Internal(_))) {
			Err(Error::new(Span::call_site(), "Cannot have Tag for Struct"))
		} else {
			Ok(None)
		};
	};

	let object = requires_object.then(|| {
		quote!(
			let __ion_object = __ion_object.get_as(__ion_cx, #variant).unwrap().unwrap();
		)
	});
	match tag {
		Some(Tag::External) => Ok(Some(quote!(
			__ion_object.set_as(__ion_cx, #variant, #ion::Object::new(__ion_cx));
			let __ion_value = __ion_object.get(__ion_cx, #variant).unwrap().unwrap();
			#object
		))),
		Some(Tag::Internal(tag)) => Ok(Some(quote!(
			__ion_object.set_as(__ion_cx, #tag, #variant);
		))),
		_ => Ok(None),
	}
}

fn map_fields(ion: &TokenStream, fields: &Fields, inherit: bool) -> Result<(Vec<Ident>, Vec<TokenStream>, bool)> {
	let mut idents = Vec::with_capacity(fields.len());
	let mut statements = Vec::with_capacity(fields.len());
	let mut requires_object = false;

	for (index, field) in fields.iter().enumerate() {
		let (ident, mut key) = field_to_ident_key(field, index);

		let mut attribute = FieldAttribute::from_attributes("ion", &field.attrs)?;
		attribute.merge(inherit);
		if let Some(name) = attribute.name {
			key = name.value();
		}
		if attribute.skip {
			continue;
		}

		let statement = if attribute.inherit {
			quote_spanned!(field.span() => #ion::conversions::ToValue::to_value(#ident, __ion_cx, __ion_value);)
		} else {
			requires_object = true;
			quote_spanned!(field.span() => __ion_object.set_as(__ion_cx, #key, #ident);)
		};

		idents.push(ident);
		statements.push(statement);
	}

	Ok((idents, statements, requires_object))
}
