/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use quote::format_ident;
use syn::punctuated::Punctuated;
use syn::{ItemFn, parse_quote};

use crate::function::parameter::Parameters;

pub(crate) fn impl_inner_fn(mut function: ItemFn, parameters: &Parameters, keep_inner: bool) -> ItemFn {
	function.attrs.push(parse_quote!(#[allow(clippy::needless_pass_by_value)]));
	function.sig.inputs = Punctuated::from_iter(parameters.to_args());
	if keep_inner {
		function.sig.ident = format_ident!("inner", span = function.sig.ident.span());
	}
	function
}
