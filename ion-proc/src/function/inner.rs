/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use quote::format_ident;
use syn::ItemFn;
use syn::punctuated::Punctuated;

use crate::function::parameter::Parameters;

pub(crate) fn impl_inner_fn(mut function: ItemFn, parameters: &Parameters, keep_inner: bool) -> ItemFn {
	function.sig.inputs = Punctuated::from_iter(parameters.to_args());
	if keep_inner {
		function.sig.ident = format_ident!("inner", span = function.sig.ident.span());
	}
	function
}
