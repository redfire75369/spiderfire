/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use base64::Engine as _;
use base64::prelude::BASE64_STANDARD;
use data_url::forgiving_base64::decode_to_vec;
use ion::string::byte::ByteString;
use ion::{Context, Error, ErrorKind, Object, Result, function_spec, js_fn};
use mozjs::jsapi::JSFunctionSpec;

const INVALID_CHARACTER_EXCEPTION: &str = "String contains an invalid character.";

#[js_fn]
fn btoa(data: ByteString) -> String {
	BASE64_STANDARD.encode(data.as_bytes())
}

#[js_fn]
fn atob(data: ByteString) -> Result<ByteString> {
	let bytes = decode_to_vec(data.as_bytes());
	let bytes = bytes.map_err(|_| Error::new(INVALID_CHARACTER_EXCEPTION, ErrorKind::Range))?;
	Ok(unsafe { ByteString::from_unchecked(bytes) })
}

const FUNCTIONS: &[JSFunctionSpec] = &[function_spec!(btoa, 1), function_spec!(atob, 1), JSFunctionSpec::ZERO];

pub fn define(cx: &Context, global: &Object) -> bool {
	unsafe { global.define_methods(cx, FUNCTIONS) }
}
