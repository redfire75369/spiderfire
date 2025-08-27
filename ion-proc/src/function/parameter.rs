/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned as _;
use syn::visit_mut::visit_type_mut;
use syn::{Error, Expr, FnArg, Ident, Pat, PatType, Receiver, Result, Stmt, Token, Type, parse_quote, parse2};

use crate::attribute::ParseAttribute as _;
use crate::attribute::function::ParameterAttribute;
use crate::utils::{pat_is_ident, path_ends_with};
use crate::visitors::{LifetimeRemover, SelfRenamer};

pub(crate) struct Parameter {
	pub(crate) pat_ty: PatType,
	convert: Option<Box<Expr>>,
}

#[derive(Clone)]
pub(crate) enum ThisKind {
	Ref { ty: Box<Type> },
	Object,
	Owned,
}

pub(crate) struct ThisParameter {
	pat_ty: PatType,
	kind: ThisKind,
}

pub(crate) struct Parameters {
	pub(crate) parameters: Vec<Parameter>,
	pub(crate) this: Option<(ThisParameter, Ident, usize)>,
	pub(crate) nargs: u16,
}

impl Parameter {
	pub(crate) fn from_arg(arg: &FnArg) -> Result<Parameter> {
		match arg {
			FnArg::Typed(pat_ty) => {
				let mut pat_ty = pat_ty.clone();
				let attribute = ParameterAttribute::from_attributes("ion", &mut pat_ty.attrs, ())?;
				Ok(Parameter { pat_ty, convert: attribute.convert })
			}
			FnArg::Receiver(_) => Err(Error::new(arg.span(), "Expected Typed Function Argument")),
		}
	}

	pub(crate) fn get_type_without_lifetimes(&self) -> Box<Type> {
		let mut ty = self.pat_ty.ty.clone();
		visit_type_mut(&mut LifetimeRemover, &mut ty);
		ty
	}

	pub(crate) fn to_statement(&self, ion: &TokenStream, ident: &Ident) -> Result<Stmt> {
		let span = self.pat_ty.span();
		let ty = self.get_type_without_lifetimes();

		let pat_ty: PatType = parse2(quote_spanned!(span => #ident: #ty))?;

		let convert;
		let convert = if let Some(convert) = &self.convert {
			convert
		} else {
			convert = parse_quote!(());
			&convert
		};

		parse2(quote_spanned!(span =>
			let #pat_ty = #ion::function::FromArgument::from_argument(&mut __accessor, #convert)?;
		))
	}
}

impl ThisParameter {
	pub(crate) fn from_arg(arg: &FnArg, class_ty: Option<&Type>) -> Result<Option<ThisParameter>> {
		match arg {
			FnArg::Typed(pat_ty) => {
				let span = pat_ty.span();
				let mut pat_ty = pat_ty.clone();
				match class_ty {
					Some(class_ty) if pat_is_ident(&pat_ty.pat, "self") => {
						visit_type_mut(&mut SelfRenamer { ty: class_ty }, &mut pat_ty.ty);
						parse_this(pat_ty, true, span).map(Some)
					}
					_ => {
						let attribute = ParameterAttribute::from_attributes("ion", &mut pat_ty.attrs, ())?;
						if attribute.this {
							return parse_this(pat_ty, class_ty.is_some(), span).map(Some);
						}
						Ok(None)
					}
				}
			}
			FnArg::Receiver(recv) => {
				if class_ty.is_none() {
					return Err(Error::new(arg.span(), "Can only have self on Class Methods"));
				}

				let class_ty = class_ty.unwrap();
				if recv.colon_token.is_some() {
					return Err(Error::new(arg.span(), "Invalid type for self"));
				}

				let Receiver {
					attrs, reference, mutability, self_token, ..
				} = recv;
				let lifetime = &reference.as_ref().unwrap().1;

				let pat_ty =
					parse2(quote_spanned!(recv.span() => #(#attrs)* #self_token: &#lifetime #mutability #class_ty))?;
				parse_this(pat_ty, true, recv.span()).map(Some)
			}
		}
	}

	pub(crate) fn to_statement(&self, ion: &TokenStream, is_class: bool) -> Result<Stmt> {
		let ThisParameter { pat_ty, kind } = self;
		let mut pat_ty = pat_ty.clone();
		pat_ty.attrs.clear();

		if is_class && pat_is_ident(&pat_ty.pat, "self") {
			pat_ty.pat = parse_quote!(self_);
		}

		match kind {
			ThisKind::Ref { ty } => {
				let mut ty = ty.clone();
				visit_type_mut(&mut LifetimeRemover, &mut ty);

				if is_class {
					parse2(quote!(
						let #pat_ty = <#ty as #ion::ClassDefinition>::get_mut_private(__cx, __this)?;
					))
				} else {
					parse2(quote!(
						let #pat_ty = <#ty as #ion::conversions::FromValue>::from_value(__cx, __accessor.this(), true, ())?;
					))
				}
			}
			ThisKind::Object => parse2(quote!(let #pat_ty = __this;)),
			ThisKind::Owned => Err(Error::new(pat_ty.span(), "This cannot be owned")),
		}
	}
}

impl Parameters {
	pub(crate) fn parse(parameters: &Punctuated<FnArg, Token![,]>, ty: Option<&Type>) -> Result<Parameters> {
		let mut nargs: u16 = 0;
		let mut this: Option<(ThisParameter, Ident, usize)> = None;

		let parameters: Vec<_> = parameters
			.iter()
			.enumerate()
			.filter_map(|(i, arg)| {
				let this_param = ThisParameter::from_arg(arg, ty);
				match this_param {
					Ok(Some(this_param)) => {
						if let Pat::Ident(pat_ident) = &*this_param.pat_ty.pat {
							if let Some(this) = &this {
								return Some(Err(Error::new(
									this.1.span(),
									"Unable to have multiple this/self parameters",
								)));
							}
							let ident = pat_ident.ident.clone();
							this = Some((this_param, ident, i));
						}
						return None;
					}
					Err(e) => return Some(Err(e)),
					_ => (),
				}

				let param = match Parameter::from_arg(arg) {
					Ok(param) => param,
					Err(e) => return Some(Err(e)),
				};
				if let Type::Path(ty) = &*param.pat_ty.ty
					&& !path_ends_with(&ty.path, "Opt")
					&& !path_ends_with(&ty.path, "Rest")
				{
					nargs = match nargs.checked_add(1) {
						Some(nargs) => nargs,
						None => return Some(Err(Error::new(arg.span(), "Function has too many arguments"))),
					}
				}
				Some(Ok(param))
			})
			.collect::<Result<_>>()?;

		Ok(Parameters { parameters, this, nargs })
	}

	pub(crate) fn to_statements(&self, ion: &TokenStream) -> Result<(Vec<Stmt>, Vec<Ident>)> {
		let mut statements = Vec::with_capacity(self.parameters.len());
		let mut idents = Vec::with_capacity(self.parameters.len() + 1);
		for (i, parameter) in self.parameters.iter().enumerate() {
			let ident = format_ident!("__ion_var{}", i);
			statements.push(parameter.to_statement(ion, &ident)?);
			idents.push(ident);
		}

		if let Some((_, ident, index)) = &self.this
			&& ident != "self"
		{
			idents.insert(*index, ident.clone());
		}

		Ok((statements, idents))
	}

	pub(crate) fn get_this_ident(&self) -> Option<Ident> {
		self.this.as_ref().map(|x| x.1.clone())
	}

	pub(crate) fn to_this_statement(&self, ion: &TokenStream, is_class: bool) -> Result<Option<Stmt>> {
		self.this.as_ref().map(|(this, _, _)| this.to_statement(ion, is_class)).transpose()
	}

	pub(crate) fn to_args(&self) -> Vec<FnArg> {
		let mut args = Vec::with_capacity(self.parameters.len() + usize::from(self.this.is_some()));

		args.extend(
			self.parameters
				.iter()
				.map(|parameter| FnArg::Typed(parameter.pat_ty.clone()))
				.collect::<Vec<_>>(),
		);

		if let Some((ThisParameter { pat_ty, .. }, _, index)) = &self.this {
			args.insert(*index, FnArg::Typed(pat_ty.clone()));
		}

		args
	}
}

pub(crate) fn parse_this(pat_ty: PatType, is_class: bool, span: Span) -> Result<ThisParameter> {
	match &*pat_ty.ty {
		Type::Reference(reference) => {
			let elem = reference.clone().elem;
			match &*elem {
				Type::Path(ty) if path_ends_with(&ty.path, "Object") => {
					Ok(ThisParameter { pat_ty, kind: ThisKind::Object })
				}
				_ => Ok(ThisParameter { pat_ty, kind: ThisKind::Ref { ty: elem } }),
			}
		}
		_ if !is_class => Ok(ThisParameter { pat_ty, kind: ThisKind::Owned }),
		_ => Err(Error::new(span, "Invalid type for self")),
	}
}
