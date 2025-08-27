/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use either::Either;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::spanned::Spanned as _;
use syn::{
	Block, Data, DeriveInput, Error, Expr, Field, Fields, Generics, ItemImpl, Result, Type, parse_quote,
	parse_quote_spanned, parse2,
};

use crate::attribute::ParseAttribute as _;
use crate::attribute::krate::crate_from_attributes;
use crate::attribute::value::{DataAttribute, DefaultValue, FieldFromAttribute, Tag, VariantAttribute};
use crate::utils::{
	add_lifetime_generic, add_trait_bounds, find_repr, format_type, path_ends_with, wrap_in_fields_group,
};
use crate::value::field_to_ident_key;

pub(crate) fn impl_from_value(mut input: DeriveInput) -> Result<ItemImpl> {
	let ion = &crate_from_attributes(&mut input.attrs);

	add_trait_bounds(&mut input.generics, &parse_quote!(#ion::conversions::FromValue));
	let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
	let mut impl_generics: Generics = parse2(quote_spanned!(impl_generics.span() => #impl_generics))?;
	add_lifetime_generic(&mut impl_generics, parse_quote!('cx));

	let attribute = DataAttribute::from_attributes("ion", &mut input.attrs, ())?;

	let repr = find_repr(&input.attrs)?;
	let name = &input.ident;

	let (body, requires_object) = impl_body(ion, input.span(), &mut input.data, name, &attribute, repr)?;

	let object = requires_object.then(|| {
		quote_spanned!(input.span() =>
			let __ion_object = #ion::Object::from_value(__ion_cx, __ion_value, true, ())?;
		)
	});

	parse2(quote_spanned!(input.span() =>
		#[automatically_derived]
		impl #impl_generics #ion::conversions::FromValue<'cx> for #name #ty_generics #where_clause {
			type Config = ();

			fn from_value(__ion_cx: &'cx #ion::Context, __ion_value: &#ion::Value, strict: bool, _: ()) -> #ion::Result<Self> {
				#object
				#body
			}
		}
	))
}

fn impl_body(
	ion: &TokenStream, span: Span, data: &mut Data, ident: &Ident, attribute: &DataAttribute, repr: Option<Ident>,
) -> Result<(Box<Block>, bool)> {
	match data {
		Data::Struct(r#struct) => match &r#struct.fields {
			Fields::Named(_) | Fields::Unnamed(_) => {
				let requirement = tag_requirement(ion, attribute.tag.0.as_ref(), None)?;
				let (idents, declarations, requires_object) =
					map_fields(ion, &mut r#struct.fields, Either::Left(attribute))?;
				let wrapped = wrap_in_fields_group(idents, &r#struct.fields);

				let block = parse2(quote_spanned!(span => {
					#requirement
					#(#declarations)*
					::std::result::Result::Ok(Self #wrapped)
				}))?;
				Ok((block, requires_object))
			}
			Fields::Unit => Ok((parse_quote_spanned!(span => { ::std::result::Result::Ok(Self) }), false)),
		},
		Data::Enum(r#enum) => {
			let mut requires_object = false;
			let mut requires_discriminant = false;

			let mut variants = Vec::with_capacity(r#enum.variants.len());

			for variant in &mut r#enum.variants {
				let variant_ident = &variant.ident;
				let variant_string = variant_ident.to_string();

				let attribute = VariantAttribute::from_attributes("ion", &mut variant.attrs, attribute)?;
				if attribute.skip {
					continue;
				}

				let handle_result = quote!(if let ::std::result::Result::Ok(success) = variant {
					return ::std::result::Result::Ok(success);
				});
				let variant: Block = match &variant.fields {
					Fields::Named(_) | Fields::Unnamed(_) => {
						let requirement = tag_requirement(ion, attribute.tag.0.as_ref(), Some(variant_string))?;
						let (idents, declarations, req_object) =
							map_fields(ion, &mut variant.fields, Either::Right(&attribute))?;
						let wrapped = wrap_in_fields_group(idents, &variant.fields);
						requires_object = requires_object || req_object;

						parse2(quote_spanned!(variant.span() => {
							let variant: #ion::Result<Self> = (|| {
								#requirement
								#(#declarations)*
								::std::result::Result::Ok(Self::#variant_ident #wrapped)
							})();
							#handle_result
						}))?
					}
					Fields::Unit => match &variant.discriminant {
						Some((_, discriminant)) => {
							if repr.is_none() {
								continue;
							}

							requires_discriminant = true;
							parse2(quote_spanned!(
								variant.fields.span() => {
									if discriminant == #discriminant {
										return ::std::result::Result::Ok(Self::#variant_ident);
									}
								}
							))?
						}
						None => parse_quote!({return ::std::result::Result::Ok(Self::#variant_ident);}),
					},
				};

				variants.push(variant);
			}

			let error = format!("Value does not match any of the variants of enum {ident}");

			let mut if_unit = None;

			if requires_discriminant && let Some(repr) = repr {
				if_unit = Some(quote_spanned!(repr.span() =>
					let discriminant: #repr = #ion::conversions::FromValue::from_value(__ion_cx, __ion_value, true, #ion::conversions::ConversionBehavior::EnforceRange)?;
				));
			}

			parse2(quote_spanned!(span => {
				#if_unit
				#(#variants)*

				::std::result::Result::Err(#ion::Error::new(#error, #ion::ErrorKind::Type))
			}))
			.map(|b| (b, requires_object))
		}
		Data::Union(_) => Err(Error::new(
			span,
			"#[derive(FromValue)] is not implemented for union types",
		)),
	}
}

fn tag_requirement(ion: &TokenStream, tag: Option<&Tag>, variant: Option<String>) -> Result<Option<TokenStream>> {
	if variant.is_none() {
		return if matches!(tag, Some(Tag::External | Tag::Internal(_))) {
			Err(Error::new(Span::call_site(), "Cannot have Tag for Struct"))
		} else {
			Ok(None)
		};
	}
	let variant = variant.unwrap();

	match tag {
		Some(Tag::External) => {
			let error = format!("Expected Object at External Tag {variant}");
			Ok(Some(quote!(
				let __ion_object: #ion::Object = __ion_object.get_as(__ion_cx, #variant, true, ())?
					.ok_or_else(|_| #ion::Error::new(#error, #ion::ErrorKind::Type))?;
			)))
		}
		Some(Tag::Internal(key)) => {
			let missing_error = format!("Expected Internal Tag key {}", key.value());
			let error = format!("Expected Internal Tag {variant} at key {}", key.value());

			Ok(Some(quote!(
				let __ion_key: ::std::string::String = __ion_object.get_as(__ion_cx, #key, true, ())?
					.ok_or_else(|| #ion::Error::new(#missing_error, #ion::ErrorKind::Type))?;
				if __ion_key != #variant {
					return Err(#ion::Error::new(#error, #ion::ErrorKind::Type));
				}
			)))
		}
		_ => Ok(None),
	}
}

fn map_fields(
	ion: &TokenStream, fields: &mut Fields, attribute: Either<&DataAttribute, &VariantAttribute>,
) -> Result<(Vec<Ident>, Vec<TokenStream>, bool)> {
	let mut requires_object = matches!(get_tag(attribute), Some(Tag::External | Tag::Internal(_)));
	let mut idents = Vec::with_capacity(fields.len());
	let mut declarations = Vec::with_capacity(fields.len());

	for (index, field) in fields.iter_mut().enumerate() {
		let (ident, mut key) = field_to_ident_key(field, index);
		let ty = &field.ty;

		let mut optional = false;
		if let Type::Path(ty) = ty
			&& path_ends_with(&ty.path, "Option")
		{
			optional = true;
		}

		let attribute = FieldFromAttribute::from_attributes("ion", &mut field.attrs, attribute)?;
		if let Some(name) = attribute.base.name {
			key = name.value();
		}
		if attribute.base.skip {
			continue;
		}

		let strict = attribute.strict;
		let convert = attribute.convert;

		let convert = convert.unwrap_or_else(|| parse_quote!(()));
		let base = field_base(
			ion,
			field,
			&ident,
			ty,
			&key,
			attribute.base.inherit,
			&mut requires_object,
			strict,
			&convert,
			attribute.parser.as_deref(),
		)?;

		let stmt = if optional {
			quote_spanned!(field.span() => #base.ok();)
		} else {
			match attribute.default.0 {
				Some(DefaultValue::Expr(expr)) => {
					if attribute.base.inherit {
						return Err(Error::new(
							field.span(),
							"Cannot have Default Expression with Inherited Field. Use a Closure with One Argument Instead",
						));
					}
					quote_spanned!(field.span() => #base.unwrap_or_else(|_| #expr);)
				}
				Some(DefaultValue::Closure(closure)) => {
					quote_spanned!(field.span() => #base.unwrap_or_else(#closure);)
				}
				Some(DefaultValue::Literal(lit)) => quote_spanned!(field.span() => #base.unwrap_or(#lit);),
				Some(DefaultValue::Default) => quote_spanned!(field.span() => #base.unwrap_or_default();),
				None => quote_spanned!(field.span() => #base?;),
			}
		};

		idents.push(ident);
		declarations.push(stmt);
	}

	Ok((idents, declarations, requires_object))
}

#[expect(clippy::too_many_arguments)]
fn field_base(
	ion: &TokenStream, field: &Field, ident: &Ident, ty: &Type, key: &str, inherit: bool, requires_object: &mut bool,
	strict: bool, convert: &Expr, parser: Option<&Expr>,
) -> Result<TokenStream> {
	if inherit {
		if *requires_object {
			return Err(Error::new(
				field.span(),
				"Inherited Field cannot be parsed from a Tagged Enum",
			));
		}
		Ok(quote_spanned!(field.span() =>
			let #ident: #ty = <#ty as #ion::conversions::FromValue>::from_value(__ion_cx, __ion_value, #strict || strict, #convert)
		))
	} else if let Some(parser) = parser {
		*requires_object = true;
		let error = format!("Expected Value at Key {key}");
		Ok(quote_spanned!(field.span() =>
			let #ident: #ty = __ion_object.get(__ion_cx, #key)?.map(#parser).transpose()?
					.ok_or_else(|| #ion::Error::new(#error, #ion::ErrorKind::Type))
		))
	} else {
		*requires_object = true;
		let error = format!("Expected Value at key {key} of Type {}", format_type(ty));
		Ok(quote_spanned!(field.span() =>
			let #ident: #ty = __ion_object.get_as(__ion_cx, #key, #strict || strict, #convert)?
					.ok_or_else(|| #ion::Error::new(#error, #ion::ErrorKind::Type))
		))
	}
}

fn get_tag<'a>(attribute: Either<&'a DataAttribute, &'a VariantAttribute>) -> Option<&'a Tag> {
	match attribute {
		Either::Left(data) => data.tag.0.as_ref(),
		Either::Right(variant) => variant.tag.0.as_ref(),
	}
}
