/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use proc_macro2::TokenStream;
use quote::quote_spanned;
use syn::spanned::Spanned as _;
use syn::{Error, Item, Result};

use crate::class::r#impl::impl_js_class_impl;
use crate::class::r#struct::impl_js_class_struct;

mod accessor;
pub(crate) mod constructor;
mod r#impl;
pub(crate) mod method;
pub(crate) mod property;
mod r#struct;

pub(super) fn impl_js_class(item: Item) -> Result<TokenStream> {
	match item {
		Item::Struct(mut r#struct) => {
			let impls = impl_js_class_struct(&mut r#struct)?;
			Ok(quote_spanned!(r#struct.span() =>
				#r#struct
				#(#impls)*
			))
		}
		Item::Impl(mut r#impl) => {
			let impls = impl_js_class_impl(&mut r#impl)?;
			Ok(quote_spanned!(r#impl.span() =>
				#r#impl
				#(#impls)*
			))
		}
		_ => Err(Error::new(item.span(), "Expected Struct or Impl Block")),
	}
}
