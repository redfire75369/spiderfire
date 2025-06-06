/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use syn::meta::ParseNestedMeta;
use syn::parse::{Parse, ParseStream};
use syn::{Expr, ExprClosure, Lit, LitStr, Result};

use crate::attribute::{ArgumentError, Optional, ParseArgument as _, ParseArgumentWith as _, ParseAttribute};

#[derive(Clone, Default)]
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

#[derive(Default)]
pub(crate) struct DataAttribute {
	pub(crate) tag: Optional<Tag>,
	pub(crate) inherit: bool,
}

impl ParseAttribute for DataAttribute {
	fn parse(&mut self, meta: &ParseNestedMeta) -> Result<()> {
		const TAG_ERROR: &str = "Data cannot have multiple `untagged`, or `tag` attributes.";

		self.tag
			.parse_argument_with(meta, Tag::Untagged, "untagged", ArgumentError::Full(TAG_ERROR))?;
		self.tag.parse_argument(meta, "tag", ArgumentError::Full(TAG_ERROR))?;
		self.inherit.parse_argument(meta, "inherit", "Data")?;

		Ok(())
	}
}

#[derive(Default)]
pub(crate) struct VariantAttribute {
	pub(crate) tag: Optional<Tag>,
	pub(crate) inherit: bool,
	pub(crate) skip: bool,
}

impl VariantAttribute {
	pub(crate) fn merge(&mut self, old_tag: Option<&Tag>, old_inherit: bool) {
		let tag = self.tag.0.take();
		self.tag = Optional(tag.or(old_tag.cloned()));
		self.inherit = self.inherit || old_inherit;
	}
}

impl ParseAttribute for VariantAttribute {
	fn parse(&mut self, meta: &ParseNestedMeta) -> Result<()> {
		const TAG_ERROR: &str = "Variant cannot have multiple `untagged`, or `tag` attributes.";

		self.tag
			.parse_argument_with(meta, Tag::Untagged, "untagged", ArgumentError::Full(TAG_ERROR))?;
		self.tag.parse_argument(meta, "tag", ArgumentError::Full(TAG_ERROR))?;
		self.inherit.parse_argument(meta, "inherit", "Variant")?;
		self.skip.parse_argument(meta, "skip", "Variant")?;

		Ok(())
	}
}

#[derive(Default)]
pub(crate) struct FieldAttribute {
	pub(crate) name: Option<LitStr>,
	pub(crate) inherit: bool,
	pub(crate) skip: bool,
}

impl FieldAttribute {
	pub(crate) fn merge(&mut self, old_inherit: bool) {
		self.inherit = self.inherit || old_inherit;
	}
}

impl ParseAttribute for FieldAttribute {
	fn parse(&mut self, meta: &ParseNestedMeta) -> Result<()> {
		self.name.parse_argument(meta, "name", "Field")?;
		self.inherit.parse_argument(meta, "inherit", "Field")?;
		self.skip.parse_argument(meta, "skip", "Field")?;

		Ok(())
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
	fn parse(&mut self, meta: &ParseNestedMeta) -> Result<()> {
		self.base.parse(meta)?;
		self.default.parse_argument(meta, "default", "Field")?;
		self.convert.parse_argument(meta, "convert", "Field")?;
		self.strict.parse_argument(meta, "strict", "Field")?;
		self.parser.parse_argument(meta, "parser", "Field")?;

		Ok(())
	}
}
