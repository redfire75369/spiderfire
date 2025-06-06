/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use idna::domain_to_ascii_strict;
use ion::function::Opt;
use ion::{ClassDefinition as _, Context, Object, Result, function_spec, js_fn};
use mozjs::jsapi::JSFunctionSpec;
use runtime::globals::url::{URL, URLSearchParams};
use runtime::module::NativeModule;

#[js_fn]
fn domain_to_ascii(domain: String, Opt(strict): Opt<bool>) -> Result<String> {
	let strict = strict.unwrap_or(false);
	let domain = if !strict {
		idna::domain_to_ascii(&domain)
	} else {
		domain_to_ascii_strict(&domain)
	};
	domain.map_err(Into::into)
}

#[js_fn]
fn domain_to_unicode(domain: String) -> String {
	idna::domain_to_unicode(&domain).0
}

const FUNCTIONS: &[JSFunctionSpec] = &[
	function_spec!(domain_to_ascii, c"domainToASCII", 0),
	function_spec!(domain_to_unicode, c"domainToUnicode", 0),
	JSFunctionSpec::ZERO,
];

pub struct UrlM;

impl<'cx> NativeModule<'cx> for UrlM {
	const NAME: &'static str = "url";
	const VARIABLE_NAME: &'static str = "url";
	const SOURCE: &'static str = include_str!("url.js");

	fn module(&self, cx: &'cx Context) -> Option<Object<'cx>> {
		let url = Object::new(cx);
		let global = Object::global(cx);

		if unsafe { url.define_methods(cx, FUNCTIONS) } {
			if let Some(global_url) = global.get(cx, "URL").unwrap() {
				url.set(cx, "URL", &global_url);
			} else {
				URL::init_class(cx, &url);
			}

			if let Some(url_search_params) = global.get(cx, "URLSearchParams").unwrap() {
				url.set(cx, "URLSearchParams", &url_search_params);
			} else {
				URLSearchParams::init_class(cx, &url);
			}

			return Some(url);
		}
		None
	}
}
