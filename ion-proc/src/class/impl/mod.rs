/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use proc_macro2::{Ident, Span, TokenStream};
use quote::{ToTokens as _, quote, quote_spanned};
use syn::spanned::Spanned as _;
use syn::{Error, FnArg, ImplItem, ImplItemFn, ItemFn, ItemImpl, Result, Type, Visibility, parse_quote, parse2};

use crate::attribute::ParseAttribute as _;
use crate::attribute::class::MethodAttribute;
use crate::attribute::krate::crate_from_attributes;
use crate::attribute::name::Name;
use crate::class::accessor::{get_accessor_name, impl_accessor, insert_accessor};
use crate::class::constructor::impl_constructor;
use crate::class::r#impl::spec::PrototypeSpecs;
use crate::class::method::{Method, MethodKind, MethodReceiver, impl_method};
use crate::class::property::{Property, PropertyType};

mod spec;

pub(super) fn impl_js_class_impl(r#impl: &mut ItemImpl) -> Result<[ItemImpl; 2]> {
	let ion = &crate_from_attributes(&mut r#impl.attrs);

	if !r#impl.generics.params.is_empty() {
		return Err(Error::new(
			r#impl.generics.span(),
			"Native Class Impls cannot have generics.",
		));
	}

	if let Some(r#trait) = &r#impl.trait_ {
		return Err(Error::new(
			r#trait.1.span(),
			"Native Class Impls cannot be for a trait.",
		));
	}

	let r#type = *r#impl.self_ty.clone();
	let mut constructor: Option<Method> = None;
	let mut specs = PrototypeSpecs::default();

	for item in &mut r#impl.items {
		match item {
			ImplItem::Const(r#const) => {
				if let Some((property, r#static)) = Property::from_const(r#const)? {
					if r#static {
						specs.properties.1.push(property);
					} else {
						specs.properties.0.push(property);
					}
				}
			}
			ImplItem::Fn(r#fn) => {
				r#fn.attrs.push(parse_quote!(#[allow(clippy::needless_pass_by_value)]));
				if let Some(parsed_constructor) = parse_class_method(ion, r#fn, &mut specs, &r#type)? {
					if let Some(constructor) = constructor.as_ref() {
						return Err(Error::new(
							r#fn.span(),
							format!(
								"Received multiple constructor implementations: {} and {}.",
								constructor.method.sig.ident, parsed_constructor.method.sig.ident
							),
						));
					}
					r#fn.attrs.push(parse_quote!(#[allow(clippy::same_name_method)]));
					constructor = Some(parsed_constructor);
				}
			}
			_ => (),
		}
	}
	specs.properties.0.push(Property {
		ty: PropertyType::String,
		ident: parse_quote!(__ION_TO_STRING_TAG),
		names: vec![Name::Symbol(
			parse_quote!(#ion::symbol::WellKnownSymbolCode::ToStringTag),
		)],
	});

	let ident: Ident = parse2(quote_spanned!(r#type.span() => #r#type))?;
	class_definition(ion, r#impl.span(), &r#type, &ident, constructor, specs)
}

fn parse_class_method(
	ion: &TokenStream, r#fn: &mut ImplItemFn, specs: &mut PrototypeSpecs, r#type: &Type,
) -> Result<Option<Method>> {
	match &r#fn.vis {
		Visibility::Public(_) => (),
		_ => return Ok(None),
	}

	let mut names = vec![];

	let attribute = MethodAttribute::from_attributes("ion", &mut r#fn.attrs, ())?;
	let MethodAttribute { name, aliases, kind, skip } = attribute;
	for alias in aliases {
		names.push(Name::String(alias));
	}
	if skip {
		return Ok(None);
	}

	let name = name.unwrap_or_else(|| {
		if kind == Some(MethodKind::Getter) || kind == Some(MethodKind::Setter) {
			Name::from_string(
				get_accessor_name(r#fn.sig.ident.to_string(), kind == Some(MethodKind::Setter)),
				r#fn.sig.ident.span(),
			)
		} else {
			Name::from_string(r#fn.sig.ident.to_string(), r#fn.sig.ident.span())
		}
	});
	names.insert(0, name.clone());

	let method: ItemFn = parse2(r#fn.to_token_stream())?;

	for input in &mut r#fn.sig.inputs {
		let attrs = match input {
			FnArg::Receiver(arg) => &mut arg.attrs,
			FnArg::Typed(arg) => &mut arg.attrs,
		};
		attrs.clear();
	}

	let class = if let Type::Path(ty) = r#type {
		ty.path.segments.last().unwrap().ident.to_string()
	} else {
		return Err(Error::new(r#type.span(), "Invalid Type for Impl"));
	};

	match kind {
		Some(MethodKind::Constructor) => {
			let constructor = impl_constructor(ion, method, r#type, &class)?;
			return Ok(Some(Method { names, ..constructor }));
		}
		Some(MethodKind::Getter) => {
			let (getter, parameters) = impl_accessor(ion, method, r#type, false, &class)?;
			let getter = Method { names, ..getter };

			if parameters.this.is_some() {
				insert_accessor(&mut specs.accessors.0, name.as_string(), Some(getter), None);
			} else {
				insert_accessor(&mut specs.accessors.1, name.as_string(), Some(getter), None);
			}
		}
		Some(MethodKind::Setter) => {
			let (setter, parameters) = impl_accessor(ion, method, r#type, true, &class)?;
			let setter = Method { names, ..setter };

			if parameters.this.is_some() {
				insert_accessor(&mut specs.accessors.0, name.as_string(), None, Some(setter));
			} else {
				insert_accessor(&mut specs.accessors.1, name.as_string(), None, Some(setter));
			}
		}
		None => {
			let (method, _) = impl_method(ion, method, r#type, &class, |_| Ok(()))?;
			let method = Method { names, ..method };

			if method.receiver == MethodReceiver::Dynamic {
				specs.methods.0.push(method);
			} else {
				specs.methods.1.push(method);
			}
		}
	}

	Ok(None)
}

fn class_definition(
	ion: &TokenStream, span: Span, r#type: &Type, ident: &Ident, constructor: Option<Method>, specs: PrototypeSpecs,
) -> Result<[ItemImpl; 2]> {
	let (spec_fns, def_fns) = specs.to_impl_fns(ion, span, ident)?;
	let constructor_function = constructor.as_ref().map(|c| &c.method);
	let functions = specs.into_functions().into_iter().map(|method| method.method);

	let mut spec_impls: ItemImpl = parse2(quote_spanned!(span => impl #r#type {
		#constructor_function
		#(#functions)*
		#(#spec_fns)*
	}))?;
	spec_impls.attrs.push(parse_quote!(#[doc(hidden)]));

	let (constructor_function, constructor_nargs) = constructor.map_or((quote!(::std::option::Option::None), 0), |c| {
		(
			quote!(::std::option::Option::Some(Self::__ion_bindings_constructor)),
			u32::from(c.nargs),
		)
	});
	let class_definition = parse2(quote_spanned!(span => impl #ion::ClassDefinition for #r#type {
		fn class() -> &'static #ion::class::NativeClass {
			static __ION_NATIVE_CLASS: &#ion::class::NativeClass = #r#type::__ion_native_class();
			__ION_NATIVE_CLASS
		}

		fn parent_prototype(cx: &#ion::Context) -> ::std::option::Option<#ion::Local<*mut ::mozjs::jsapi::JSObject>> {
			#r#type::__ion_parent_prototype(cx)
		}

		fn constructor() -> (::std::option::Option<#ion::function::NativeFunction>, ::core::primitive::u32) {
			(#constructor_function, #constructor_nargs)
		}

		#(#def_fns)*
	}))?;

	Ok([spec_impls, class_definition])
}
