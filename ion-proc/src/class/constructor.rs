/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use proc_macro2::TokenStream;
use syn::{ItemFn, Result, Type};

use crate::class::method::{Method, MethodReceiver};
use crate::function::wrapper::impl_wrapper_fn;
use crate::function::{check_abi, set_signature};

pub(super) fn impl_constructor(ion: &TokenStream, mut constructor: ItemFn, ty: &Type) -> Result<Method> {
	let (wrapper, parameters) = impl_wrapper_fn(ion, constructor.clone(), Some(ty), true)?;

	check_abi(&mut constructor)?;
	set_signature(&mut constructor)?;
	constructor.attrs.clear();

	let body = parse_quote!({
		#wrapper

		let cx = &#ion::Context::new_unchecked(cx);
		let args = &mut #ion::Arguments::new(cx, argc, vp);

		unsafe {
			#ion::class::construct_native_object::<#ty, _>(
				cx, args, |cx, args, this| unsafe { wrapper(cx, args, this) }
			)
		}
	});
	constructor.block = body;
	constructor.sig.ident = format_ident!("__ion_bindings_constructor", span = constructor.sig.ident.span());

	let method = Method {
		receiver: MethodReceiver::Static,
		method: constructor,
		nargs: parameters.nargs,
		names: vec![],
	};
	Ok(method)
}
