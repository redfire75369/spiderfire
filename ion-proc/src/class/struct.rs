/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{Error, ItemImpl, ItemStruct, Member, Path, Result, Token, Type, parse_quote, parse2};

use crate::attribute::ParseAttribute;
use crate::attribute::class::ClassAttribute;
use crate::attribute::krate::crate_from_attributes;
use crate::utils::{new_token, path_ends_with};

pub(super) fn impl_js_class_struct(r#struct: &mut ItemStruct) -> Result<[ItemImpl; 7]> {
	let ion = &crate_from_attributes(&mut r#struct.attrs);

	let repr_c = r#struct.attrs.iter().fold(Ok(false), |acc, attr| {
		if attr.path().is_ident("repr") {
			return match attr.parse_args::<Ident>() {
				Ok(ident) if ident == "C" => Ok(true),
				_ => Err(Error::new(attr.span(), "Only C Representations are allowed.")),
			};
		}
		acc
	})?;
	if !repr_c {
		r#struct.attrs.push(parse_quote!(#[repr(C)]));
	}

	let traceable = r#struct.attrs.iter().any(|attr| {
		if attr.path().is_ident("derive") {
			if let Ok(paths) = attr.parse_args_with(Punctuated::<Path, Token![,]>::parse_terminated) {
				return paths.iter().any(|path| path_ends_with(path, "Traceable"));
			}
		}
		false
	});
	if !traceable {
		r#struct.attrs.push(parse_quote!(#[derive(#ion::Traceable)]));
	}

	let attribute = ClassAttribute::from_attributes_mut("ion", &mut r#struct.attrs)?;

	if !r#struct.generics.params.is_empty() {
		return Err(Error::new(
			r#struct.generics.span(),
			"Native Class Structs cannot have generics.",
		));
	}

	let name = if let Some(name) = attribute.name {
		name.value()
	} else {
		r#struct.ident.to_string()
	};

	let ident = &r#struct.ident;
	let r#type: Type = parse2(quote_spanned!(ident.span() => #ident))?;

	let (super_field, super_type) = if let Some(field) = r#struct.fields.iter().next() {
		(Member::Named(field.ident.as_ref().unwrap().clone()), field.ty.clone())
	} else {
		return Err(Error::new(
			r#struct.span(),
			"Native Class Structs must have at least a reflector field.",
		));
	};

	if let Type::Path(ty) = &super_type {
		if ty.path.segments.iter().any(|segment| !segment.arguments.is_empty()) {
			return Err(Error::new(super_type.span(), "Superclass Type must not have generics."));
		}
	} else {
		return Err(Error::new(super_type.span(), "Superclass Type must be a path."));
	}

	class_impls(ion, r#struct.span(), &name, &r#type, &super_field, &super_type)
}

fn class_impls(
	ion: &TokenStream, span: Span, name: &str, r#type: &Type, super_field: &Member, super_type: &Type,
) -> Result<[ItemImpl; 7]> {
	let from_value = impl_from_value(ion, span, r#type, false)?;
	let from_value_mut = impl_from_value(ion, span, r#type, true)?;

	let derived_from =
		parse2(quote_spanned!(span => unsafe impl #ion::class::DerivedFrom<#super_type> for #r#type {}))?;
	let castable = parse2(quote_spanned!(span => impl #ion::class::Castable for #r#type {}))?;

	let native_object = parse2(quote_spanned!(span => impl #ion::class::NativeObject for #r#type {
		fn reflector(&self) -> &#ion::class::Reflector {
			#ion::class::NativeObject::reflector(&self.#super_field)
		}
	}))?;

	let none = quote!(::std::option::Option::None);
	let name = format!("{name}\0");

	let mut class_impl: ItemImpl = parse2(quote_spanned!(span => impl #r#type {
		pub const fn __ion_native_prototype_chain() -> #ion::class::PrototypeChain {
			const ION_TYPE_ID: #ion::class::TypeIdWrapper<#r#type> = #ion::class::TypeIdWrapper::new();
			#super_type::__ion_native_prototype_chain().push(&ION_TYPE_ID)
		}

		pub const fn __ion_native_class() -> &'static #ion::class::NativeClass {
			const ION_CLASS_OPERATIONS: ::mozjs::jsapi::JSClassOps = ::mozjs::jsapi::JSClassOps {
				addProperty: #none,
				delProperty: #none,
				enumerate: #none,
				newEnumerate: #none,
				resolve: #none,
				mayResolve: #none,
				finalize: ::std::option::Option::Some(#ion::class::finalise_native_object_operation::<#r#type>),
				call: #none,
				construct: #none,
				trace: ::std::option::Option::Some(#ion::class::trace_native_object_operation::<#r#type>),
			};

			const ION_NATIVE_CLASS: #ion::class::NativeClass = #ion::class::NativeClass {
				base: ::mozjs::jsapi::JSClass {
					name: #name.as_ptr().cast(),
					flags: #ion::object::class_reserved_slots(1) | ::mozjs::jsapi::JSCLASS_BACKGROUND_FINALIZE,
					cOps: ::std::ptr::from_ref(&ION_CLASS_OPERATIONS),
					spec: ::std::ptr::null_mut(),
					ext: ::std::ptr::null_mut(),
					oOps: ::std::ptr::null_mut(),
				},
				prototype_chain: #r#type::__ion_native_prototype_chain(),
			};

			&ION_NATIVE_CLASS
		}

		pub const __ION_TO_STRING_TAG: &'static str = #name;
	}))?;
	class_impl.attrs.push(parse_quote!(#[doc(hidden)]));

	let is_reflector = matches!(super_type, Type::Path(ty) if path_ends_with(&ty.path, "Reflector"));
	let parent_proto = if is_reflector {
		quote_spanned!(super_type.span() => ::std::option::Option::None)
	} else {
		quote_spanned!(super_type.span() =>
			let infos = unsafe { &mut (*cx.get_inner_data().as_ptr()).class_infos };
			let info = infos.get(&::core::any::TypeId::of::<#super_type>()).expect("Uninitialised Class");
			::std::option::Option::Some(cx.root(info.prototype.get()))
		)
	};
	let mut parent_impl: ItemImpl = parse2(quote_spanned!(super_type.span() => impl #r#type {
		#[allow(unused_variables)]
		pub fn __ion_parent_prototype(cx: &#ion::Context) -> ::std::option::Option<#ion::Local<*mut ::mozjs::jsapi::JSObject>> {
			#parent_proto
		}
	}))?;
	parent_impl.attrs.push(parse_quote!(#[doc(hidden)]));

	Ok([
		from_value,
		from_value_mut,
		derived_from,
		castable,
		native_object,
		class_impl,
		parent_impl,
	])
}

fn impl_from_value(ion: &TokenStream, span: Span, r#type: &Type, mutable: bool) -> Result<ItemImpl> {
	let (function, mutable) = if mutable {
		(quote!(get_mut_private), Some(new_token![mut]))
	} else {
		(quote!(get_private), None)
	};

	parse2(
		quote_spanned!(span => impl<'cx> #ion::conversions::FromValue<'cx> for &'cx #mutable #r#type {
			type Config = ();

			fn from_value(cx: &'cx #ion::Context, value: &#ion::Value, strict: ::core::primitive::bool, _: ()) -> #ion::Result<&'cx #mutable #r#type> {
				let object = #ion::Object::from_value(cx, value, strict, ())?;
				<#r#type as #ion::class::ClassDefinition>::#function(cx, &object)
			}
		}),
	)
}
