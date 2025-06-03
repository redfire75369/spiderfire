/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use proc_macro2::Ident;
use syn::meta::ParseNestedMeta;
use syn::parse::Parse;
use syn::punctuated::Punctuated;
use syn::{Attribute, Result, Token, bracketed};

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
	) -> Result<()>;

	fn parse_argument_with<'a>(
		&mut self, meta: &ParseNestedMeta, with: Self::With, key: &str, error: impl Into<ArgumentError<'a>>,
	) -> Result<()> {
		if meta.path.is_ident(key) {
			self.handle_argument_with(meta, with, key, error)?;
		}
		Ok(())
	}
}

pub(crate) trait ParseArgument: ParseArgumentWith {
	fn handle_argument<'a>(
		&mut self, meta: &ParseNestedMeta, key: &str, error: impl Into<ArgumentError<'a>>,
	) -> Result<()>;

	fn parse_argument<'a>(
		&mut self, meta: &ParseNestedMeta, key: &str, error: impl Into<ArgumentError<'a>>,
	) -> Result<()> {
		if meta.path.is_ident(key) {
			self.handle_argument(meta, key, error)?;
		}
		Ok(())
	}
}

impl ParseArgumentWith for bool {
	type With = bool;

	fn handle_argument_with<'a>(
		&mut self, meta: &ParseNestedMeta, with: bool, key: &str, error: impl Into<ArgumentError<'a>>,
	) -> Result<()> {
		if *self {
			error.into().error(meta, key)?;
		}
		*self = with;
		Ok(())
	}
}

impl ParseArgument for bool {
	fn handle_argument<'a>(
		&mut self, meta: &ParseNestedMeta, key: &str, error: impl Into<ArgumentError<'a>>,
	) -> Result<()> {
		self.handle_argument_with(meta, true, key, error)
	}
}

impl<T> ParseArgumentWith for Option<T> {
	type With = T;

	fn handle_argument_with<'a>(
		&mut self, meta: &ParseNestedMeta, with: T, key: &str, error: impl Into<ArgumentError<'a>>,
	) -> Result<()> {
		if self.is_some() {
			error.into().error(meta, key)?;
		}
		*self = Some(with);
		Ok(())
	}
}

impl<T: Parse> ParseArgument for Option<T> {
	fn handle_argument<'a>(
		&mut self, meta: &ParseNestedMeta, key: &str, error: impl Into<ArgumentError<'a>>,
	) -> Result<()> {
		let _: Token![=] = meta.input.parse()?;
		let argument = meta.input.parse()?;
		self.handle_argument_with(meta, argument, key, error)
	}
}

impl<T> ParseArgumentWith for Optional<T> {
	type With = T;

	fn handle_argument_with<'a>(
		&mut self, meta: &ParseNestedMeta, with: T, key: &str, error: impl Into<ArgumentError<'a>>,
	) -> Result<()> {
		if self.0.is_some() {
			error.into().error(meta, key)?;
		}
		self.0 = Some(with);
		Ok(())
	}
}

impl<T: Parse + Default> ParseArgument for Optional<T> {
	fn handle_argument<'a>(
		&mut self, meta: &ParseNestedMeta, key: &str, error: impl Into<ArgumentError<'a>>,
	) -> Result<()> {
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
	) -> Result<()> {
		if !self.is_empty() {
			error.into().error(meta, key)?;
		}
		self.extend(with);
		Ok(())
	}
}

impl<T: Parse> ParseArgument for Vec<T> {
	fn handle_argument<'a>(
		&mut self, meta: &ParseNestedMeta, key: &str, error: impl Into<ArgumentError<'a>>,
	) -> Result<()> {
		let _: Token![=] = meta.input.parse()?;
		let inner;
		bracketed!(inner in meta.input);
		let value = inner.parse_terminated(T::parse, Token![,])?;
		self.handle_argument_with(meta, value, key, error)
	}
}

pub(crate) trait ParseAttribute: Default {
	fn parse(&mut self, meta: &ParseNestedMeta) -> Result<()>;

	fn from_attributes<I: ?Sized>(path: &I, attrs: &[Attribute]) -> Result<Self>
	where
		Ident: PartialEq<I>,
	{
		let mut attribute = Self::default();
		for attr in attrs {
			if attr.path().is_ident(path) {
				attr.parse_nested_meta(|meta| attribute.parse(&meta))?;
			}
		}
		Ok(attribute)
	}

	fn from_attributes_mut<I: ?Sized>(path: &I, attrs: &mut Vec<Attribute>) -> Result<Self>
	where
		Ident: PartialEq<I>,
	{
		let mut indices = Vec::new();
		let mut attribute = Self::default();
		for (i, attr) in attrs.iter().enumerate() {
			if attr.path().is_ident(path) {
				attr.parse_nested_meta(|meta| attribute.parse(&meta))?;
				indices.push(i);
				break;
			}
		}
		while let Some(index) = indices.pop() {
			attrs.remove(index);
		}
		Ok(attribute)
	}
}
