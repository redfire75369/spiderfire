/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use proc_macro2::{Ident, TokenStream};
use quote::quote;
use syn::meta::ParseNestedMeta;
use syn::parse::Parse;
use syn::punctuated::Punctuated;
use syn::{Attribute, Meta, MetaList, Path, Result, Token, bracketed};

pub(crate) mod class;
pub(crate) mod function;
pub(crate) mod krate;
pub(crate) mod name;
pub(crate) mod property;
pub(crate) mod trace;
pub(crate) mod value;

#[derive(Copy, Clone, Debug)]
pub(crate) struct Optional<T>(pub(crate) Option<T>);

impl<T> Default for Optional<T> {
	fn default() -> Optional<T> {
		Optional(None)
	}
}

pub(crate) enum ArgumentError<'a> {
	Kind(&'a str),
	Full(&'a str),
	None,
}

impl ArgumentError<'_> {
	fn error(self, meta: &ParseNestedMeta, key: &str) -> Result<()> {
		match self {
			ArgumentError::Kind(kind) => Err(meta.error(format!("{kind} cannot have multiple `{key}` attributes."))),
			ArgumentError::Full(error) => Err(meta.error(error)),
			ArgumentError::None => Ok(()),
		}
	}
}

impl<'a> From<&'a str> for ArgumentError<'a> {
	fn from(kind: &'a str) -> ArgumentError<'a> {
		ArgumentError::Kind(kind)
	}
}

impl<'a> From<Option<&'a str>> for ArgumentError<'a> {
	fn from(kind: Option<&'a str>) -> ArgumentError<'a> {
		match kind {
			Some(kind) => ArgumentError::from(kind),
			None => ArgumentError::None,
		}
	}
}

pub(crate) trait ParseArgumentWith {
	type With;

	fn handle_argument_with<'a>(
		&mut self, meta: &ParseNestedMeta, with: Self::With, key: &str, error: impl Into<ArgumentError<'a>>,
	) -> Result<bool>;

	fn parse_argument_with<'a>(
		&mut self, meta: &ParseNestedMeta, with: Self::With, key: &str, error: impl Into<ArgumentError<'a>>,
	) -> Result<bool> {
		if meta.path.is_ident(key) {
			return self.handle_argument_with(meta, with, key, error);
		}
		Ok(false)
	}
}

pub(crate) trait ParseArgument: ParseArgumentWith {
	fn handle_argument<'a>(
		&mut self, meta: &ParseNestedMeta, key: &str, error: impl Into<ArgumentError<'a>>,
	) -> Result<bool>;

	fn parse_argument<'a>(
		&mut self, meta: &ParseNestedMeta, key: &str, error: impl Into<ArgumentError<'a>>,
	) -> Result<bool> {
		if meta.path.is_ident(key) {
			return self.handle_argument(meta, key, error);
		}
		Ok(false)
	}
}

impl ParseArgumentWith for bool {
	type With = bool;

	fn handle_argument_with<'a>(
		&mut self, meta: &ParseNestedMeta, with: bool, key: &str, error: impl Into<ArgumentError<'a>>,
	) -> Result<bool> {
		if *self {
			error.into().error(meta, key)?;
			return Ok(false);
		}
		*self = with;
		Ok(true)
	}
}

impl ParseArgument for bool {
	fn handle_argument<'a>(
		&mut self, meta: &ParseNestedMeta, key: &str, error: impl Into<ArgumentError<'a>>,
	) -> Result<bool> {
		self.handle_argument_with(meta, true, key, error)
	}
}

impl<T> ParseArgumentWith for Option<T> {
	type With = T;

	fn handle_argument_with<'a>(
		&mut self, meta: &ParseNestedMeta, with: T, key: &str, error: impl Into<ArgumentError<'a>>,
	) -> Result<bool> {
		if self.is_some() {
			error.into().error(meta, key)?;
			return Ok(false);
		}
		*self = Some(with);
		Ok(true)
	}
}

impl<T: Parse> ParseArgument for Option<T> {
	fn handle_argument<'a>(
		&mut self, meta: &ParseNestedMeta, key: &str, error: impl Into<ArgumentError<'a>>,
	) -> Result<bool> {
		let _: Token![=] = meta.input.parse()?;
		let argument = meta.input.parse()?;
		self.handle_argument_with(meta, argument, key, error)
	}
}

impl<T> ParseArgumentWith for Optional<T> {
	type With = T;

	fn handle_argument_with<'a>(
		&mut self, meta: &ParseNestedMeta, with: T, key: &str, error: impl Into<ArgumentError<'a>>,
	) -> Result<bool> {
		if self.0.is_some() {
			error.into().error(meta, key)?;
			return Ok(false);
		}
		self.0 = Some(with);
		Ok(true)
	}
}

impl<T: Parse + Default> ParseArgument for Optional<T> {
	fn handle_argument<'a>(
		&mut self, meta: &ParseNestedMeta, key: &str, error: impl Into<ArgumentError<'a>>,
	) -> Result<bool> {
		let eq: Option<Token![=]> = meta.input.parse()?;
		let argument = eq.map(|_| meta.input.parse()).transpose()?.unwrap_or_default();
		self.handle_argument_with(meta, argument, key, error)
	}
}

impl<T> ParseArgumentWith for Vec<T> {
	type With = Punctuated<T, Token![,]>;

	fn handle_argument_with<'a>(
		&mut self, meta: &ParseNestedMeta, with: Punctuated<T, Token![,]>, key: &str,
		error: impl Into<ArgumentError<'a>>,
	) -> Result<bool> {
		if !self.is_empty() {
			error.into().error(meta, key)?;
			return Ok(false);
		}
		self.extend(with);
		Ok(true)
	}
}

impl<T: Parse> ParseArgument for Vec<T> {
	fn handle_argument<'a>(
		&mut self, meta: &ParseNestedMeta, key: &str, error: impl Into<ArgumentError<'a>>,
	) -> Result<bool> {
		let _: Token![=] = meta.input.parse()?;
		let inner;
		bracketed!(inner in meta.input);
		let value = inner.parse_terminated(T::parse, Token![,])?;
		self.handle_argument_with(meta, value, key, error)
	}
}

pub(crate) trait ParseAttribute: Default {
	type Parent<'a>: Copy;

	fn from_parent(_parent: Self::Parent<'_>) -> Self {
		Self::default()
	}

	fn merge(&mut self, _parent: Self::Parent<'_>) {}

	fn parse(&mut self, meta: &ParseNestedMeta) -> Result<bool>;

	fn from_attributes<I: ?Sized>(path: &I, attributes: &mut Vec<Attribute>, parent: Self::Parent<'_>) -> Result<Self>
	where
		Ident: PartialEq<I>,
	{
		let mut data = Self::from_parent(parent);

		attributes.retain_mut(|attribute| {
			if !attribute.path().is_ident(path) {
				return true;
			}

			let mut raw: Vec<(Path, TokenStream)> = Vec::new();
			let mut parsed = Vec::new();

			let _ = attribute.parse_nested_meta(|meta| {
				raw.push((meta.path.clone(), meta.input.fork().parse()?));
				if data.parse(&meta)? {
					parsed.push(raw.len());
				}

				Ok(())
			});

			if parsed.len() == raw.len() {
				return false;
			}

			if let Meta::List(MetaList { tokens, .. }) = &mut attribute.meta {
				let raw = raw
					.into_iter()
					.enumerate()
					.filter_map(|(i, (k, v))| (!parsed.contains(&i)).then(|| quote!(#k #v)));
				*tokens = quote!(#(#raw),*);
			}

			true
		});

		data.merge(parent);
		Ok(data)
	}
}
