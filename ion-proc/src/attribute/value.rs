/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use either::Either;
use syn::meta::ParseNestedMeta;
use syn::parse::{Parse, ParseStream};
use syn::{Expr, ExprClosure, Lit, LitStr, Result};

use crate::attribute::{ArgumentError, Optional, ParseArgument as _, ParseArgumentWith as _, ParseAttribute};

#[derive(Clone, Debug, Default)]
pub(crate) enum Tag {
	Untagged,
	#[default]
	External,
	Internal(LitStr),
}

impl Parse for Tag {
	fn parse(input: ParseStream) -> Result<Tag> {
		let tag: Option<_> = input.parse()?;
		Ok(tag.map_or(Tag::External, Tag::Internal))
	}
}

#[derive(Default)]
pub(crate) enum DefaultValue {
	#[default]
	Default,
	Literal(Lit),
	Closure(ExprClosure),
	Expr(Box<Expr>),
}

impl Parse for DefaultValue {
	fn parse(input: ParseStream) -> Result<DefaultValue> {
		let expr: Expr = input.parse()?;
		match expr {
			Expr::Lit(lit) => Ok(DefaultValue::Literal(lit.lit)),
			Expr::Closure(closure) => Ok(DefaultValue::Closure(closure)),
			expr => Ok(DefaultValue::Expr(Box::new(expr))),
		}
	}
}

#[derive(Debug, Default)]
pub(crate) struct DataAttribute {
	pub(crate) tag: Optional<Tag>,
	pub(crate) inherit: bool,
}

impl ParseAttribute for DataAttribute {
	type Parent<'a> = ();

	fn parse(&mut self, meta: &ParseNestedMeta) -> Result<bool> {
		const TAG_ERROR: &str = "Data cannot have multiple `untagged`, or `tag` attributes.";

		let success = self
			.tag
			.parse_argument_with(meta, Tag::Untagged, "untagged", ArgumentError::Full(TAG_ERROR))?
			|| self.tag.parse_argument(meta, "tag", ArgumentError::Full(TAG_ERROR))?
			|| self.inherit.parse_argument(meta, "inherit", "Data")?;

		Ok(success)
	}
}

#[derive(Debug, Default)]
pub(crate) struct VariantAttribute {
	pub(crate) tag: Optional<Tag>,
	pub(crate) inherit: bool,
	pub(crate) skip: bool,
}

impl ParseAttribute for VariantAttribute {
	type Parent<'a> = &'a DataAttribute;

	fn from_parent(parent: Self::Parent<'_>) -> VariantAttribute {
		VariantAttribute {
			inherit: parent.inherit,
			..VariantAttribute::default()
		}
	}

	fn merge(&mut self, parent: Self::Parent<'_>) {
		if self.tag.0.is_none() {
			self.tag = parent.tag.clone();
		}
	}

	fn parse(&mut self, meta: &ParseNestedMeta) -> Result<bool> {
		const TAG_ERROR: &str = "Variant cannot have multiple `untagged`, or `tag` attributes.";

		let success = self
			.tag
			.parse_argument_with(meta, Tag::Untagged, "untagged", ArgumentError::Full(TAG_ERROR))?
			|| self.tag.parse_argument(meta, "tag", ArgumentError::Full(TAG_ERROR))?
			|| self.inherit.parse_argument(meta, "inherit", "Variant")?
			|| self.skip.parse_argument(meta, "skip", "Variant")?;

		Ok(success)
	}
}

#[derive(Default)]
pub(crate) struct FieldAttribute {
	pub(crate) name: Option<LitStr>,
	pub(crate) inherit: bool,
	pub(crate) skip: bool,
}

impl ParseAttribute for FieldAttribute {
	type Parent<'a> = Either<&'a DataAttribute, &'a VariantAttribute>;

	fn from_parent(parent: Self::Parent<'_>) -> FieldAttribute {
		let mut attribute = FieldAttribute::default();
		match parent {
			Either::Left(data) => {
				attribute.inherit = data.inherit;
			}
			Either::Right(variant) => {
				attribute.inherit = variant.inherit;
				attribute.skip = variant.skip;
			}
		}
		attribute
	}

	fn parse(&mut self, meta: &ParseNestedMeta) -> Result<bool> {
		let success = self.name.parse_argument(meta, "name", "Field")?
			|| self.inherit.parse_argument(meta, "inherit", "Field")?
			|| self.skip.parse_argument(meta, "skip", "Field")?;

		Ok(success)
	}
}

#[derive(Default)]
pub(crate) struct FieldFromAttribute {
	pub(crate) base: FieldAttribute,
	pub(crate) convert: Option<Box<Expr>>,
	pub(crate) strict: bool,
	pub(crate) default: Optional<DefaultValue>,
	pub(crate) parser: Option<Box<Expr>>,
}

impl ParseAttribute for FieldFromAttribute {
	type Parent<'a> = Either<&'a DataAttribute, &'a VariantAttribute>;

	fn from_parent(parent: Self::Parent<'_>) -> Self {
		FieldFromAttribute {
			base: FieldAttribute::from_parent(parent),
			..FieldFromAttribute::default()
		}
	}

	fn parse(&mut self, meta: &ParseNestedMeta) -> Result<bool> {
		let success = self.base.parse(meta)?
			|| self.default.parse_argument(meta, "default", "Field")?
			|| self.convert.parse_argument(meta, "convert", "Field")?
			|| self.strict.parse_argument(meta, "strict", "Field")?
			|| self.parser.parse_argument(meta, "parser", "Field")?;

		Ok(success)
	}
}
