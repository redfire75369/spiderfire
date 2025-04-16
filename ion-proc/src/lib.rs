/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

#[macro_use]
extern crate quote;
#[macro_use]
extern crate syn;

use proc_macro::TokenStream;
use quote::ToTokens;

use crate::class::impl_js_class;
use crate::function::impl_js_fn;
use crate::trace::impl_trace;
use crate::value::{impl_from_value, impl_to_value};

pub(crate) mod attribute;
pub(crate) mod class;
pub(crate) mod function;
mod trace;
pub(crate) mod utils;
pub(crate) mod value;
pub(crate) mod visitors;

#[proc_macro_attribute]
pub fn js_fn(_attr: TokenStream, stream: TokenStream) -> TokenStream {
	match impl_js_fn(parse_macro_input!(stream)) {
		Ok(function) => function.into_token_stream().into(),
		Err(error) => error.to_compile_error().into(),
	}
}

#[proc_macro_attribute]
pub fn js_class(_attr: TokenStream, stream: TokenStream) -> TokenStream {
	match impl_js_class(parse_macro_input!(stream)) {
		Ok(module) => module.into_token_stream().into(),
		Err(error) => error.to_compile_error().into(),
	}
}

#[proc_macro_derive(Traceable, attributes(trace))]
pub fn trace(input: TokenStream) -> TokenStream {
	match impl_trace(parse_macro_input!(input)) {
		Ok(trace) => trace.into_token_stream().into(),
		Err(error) => error.to_compile_error().into(),
	}
}

#[proc_macro_derive(FromValue, attributes(ion))]
pub fn from_value(input: TokenStream) -> TokenStream {
	match impl_from_value(parse_macro_input!(input)) {
		Ok(from_value) => from_value.into_token_stream().into(),
		Err(error) => error.to_compile_error().into(),
	}
}

#[proc_macro_derive(ToValue, attributes(ion))]
pub fn to_value(input: TokenStream) -> TokenStream {
	match impl_to_value(parse_macro_input!(input)) {
		Ok(to_value) => to_value.into_token_stream().into(),
		Err(error) => error.to_compile_error().into(),
	}
}
