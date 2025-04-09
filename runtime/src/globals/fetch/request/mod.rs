/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::str::FromStr;

use http::{HeaderMap, Method};
use ion::class::{NativeObject, Reflector};
use ion::function::Opt;
use ion::typedarray::{ArrayBufferWrapper, Uint8ArrayWrapper};
use ion::{ClassDefinition, Context, Error, ErrorKind, Object, Promise, Result, TracedHeap};
use mozjs::jsapi::{Heap, JSObject};
pub use options::*;
use url::Url;

use crate::globals::abort::AbortSignal;
use crate::globals::fetch::Headers;
use crate::globals::fetch::body::FetchBody;
use crate::globals::fetch::header::HeadersKind;
use crate::promise::future_to_promise;

mod options;

#[derive(FromValue)]
#[ion(inherit)]
pub enum RequestInfo<'cx> {
	Request(&'cx Request),
	String(String),
}

#[js_class]
pub struct Request {
	reflector: Reflector,

	pub(crate) headers: Box<Heap<*mut JSObject>>,
	pub(crate) body: Option<FetchBody>,

	#[trace(no_trace)]
	pub(crate) method: Method,
	#[trace(no_trace)]
	pub(crate) url: Url,
	#[trace(no_trace)]
	pub(crate) locations: Vec<Url>,

	pub(crate) referrer: Referrer,
	pub(crate) referrer_policy: ReferrerPolicy,

	pub(crate) mode: RequestMode,
	pub(crate) credentials: RequestCredentials,
	pub(crate) cache: RequestCache,
	pub(crate) redirect: RequestRedirect,

	pub(crate) integrity: String,

	pub(crate) unsafe_request: bool,
	pub(crate) keepalive: bool,

	pub(crate) client_window: bool,
	pub(crate) signal_object: Box<Heap<*mut JSObject>>,
}

#[js_class]
impl Request {
	#[ion(constructor)]
	pub fn constructor(cx: &Context, info: RequestInfo, Opt(init): Opt<RequestInit>) -> Result<Request> {
		let mut fallback_cors = false;

		let mut request = match info {
			RequestInfo::Request(request) => request.clone(),
			RequestInfo::String(url) => {
				let url = Url::from_str(&url)?;
				if url.username() != "" || url.password().is_some() {
					return Err(Error::new("Received URL with embedded credentials", ErrorKind::Type));
				}

				fallback_cors = true;

				Request {
					reflector: Reflector::default(),

					headers: Box::default(),
					body: None,

					method: Method::GET,
					url: url.clone(),
					locations: vec![url],

					referrer: Referrer::default(),
					referrer_policy: ReferrerPolicy::default(),

					mode: RequestMode::default(),
					credentials: RequestCredentials::default(),
					cache: RequestCache::default(),
					redirect: RequestRedirect::default(),

					integrity: String::new(),

					unsafe_request: false,
					keepalive: false,

					client_window: true,
					signal_object: Heap::boxed(AbortSignal::new_object(cx, Box::default())),
				}
			}
		};

		let mut headers = None;
		let mut body = None;

		if let Some(init) = init {
			if let Some(window) = init.window {
				if window.is_null() {
					request.client_window = false;
				} else {
					return Err(Error::new("Received non-null window type", ErrorKind::Type));
				}
			}

			if request.mode == RequestMode::Navigate {
				request.mode = RequestMode::SameOrigin;
			}

			if let Some(referrer) = init.referrer {
				request.referrer = referrer;
			}
			if let Some(policy) = init.referrer_policy {
				request.referrer_policy = policy;
			}

			let mode = init.mode.or(fallback_cors.then_some(RequestMode::Cors));
			if let Some(mode) = mode {
				if mode == RequestMode::Navigate {
					return Err(Error::new("Received 'navigate' mode", ErrorKind::Type));
				}
				request.mode = mode;
			}

			if let Some(credentials) = init.credentials {
				request.credentials = credentials;
			}
			if let Some(cache) = init.cache {
				request.cache = cache;
			}
			if let Some(redirect) = init.redirect {
				request.redirect = redirect;
			}
			if let Some(integrity) = init.integrity {
				request.integrity = integrity;
			}
			if let Some(keepalive) = init.keepalive {
				request.keepalive = keepalive;
			}

			if let Some(signal_object) = init.signal {
				request.signal_object.set(signal_object);
			}

			if let Some(mut method) = init.method {
				method.make_ascii_uppercase();
				let method = Method::from_str(&method)?;
				if method == Method::CONNECT || method == Method::TRACE {
					return Err(Error::new("Received invalid request method", ErrorKind::Type));
				}
				request.method = method;
			}

			headers = init.headers;
			body = init.body;
		}

		if request.cache == RequestCache::OnlyIfCached && request.mode != RequestMode::SameOrigin {
			return Err(Error::new(
				"Request cache mode 'only-if-cached' can only be used with request mode 'same-origin'",
				ErrorKind::Type,
			));
		}

		if request.mode == RequestMode::NoCors && !matches!(request.method, Method::GET | Method::HEAD | Method::POST) {
			return Err(Error::new("Invalid request method", ErrorKind::Type));
		}

		let kind = if request.mode == RequestMode::NoCors {
			HeadersKind::RequestNoCors
		} else {
			HeadersKind::Request
		};

		let mut headers = if let Some(headers) = headers {
			headers.into_headers(HeaderMap::new(), kind)?
		} else {
			Headers {
				reflector: Reflector::default(),
				headers: HeaderMap::new(),
				kind,
			}
		};

		if let Some(body) = body {
			body.add_content_type_header(&mut headers.headers);
			request.body = Some(body);
		}
		request.headers.set(Headers::new_object(cx, Box::new(headers)));

		Ok(request)
	}

	#[ion(get)]
	pub fn get_method(&self) -> String {
		self.method.to_string()
	}

	#[ion(get)]
	pub fn get_url(&self) -> String {
		self.url.to_string()
	}

	#[ion(get)]
	pub fn get_headers(&self) -> *mut JSObject {
		self.headers.get()
	}

	#[ion(get)]
	pub fn get_destination(&self) -> String {
		String::new()
	}

	#[ion(get)]
	pub fn get_referrer(&self) -> String {
		self.referrer.to_string()
	}

	#[ion(get)]
	pub fn get_referrer_policy(&self) -> String {
		self.referrer.to_string()
	}

	#[ion(get)]
	pub fn get_mode(&self) -> String {
		self.mode.to_string()
	}

	#[ion(get)]
	pub fn get_credentials(&self) -> String {
		self.credentials.to_string()
	}

	#[ion(get)]
	pub fn get_cache(&self) -> String {
		self.cache.to_string()
	}

	#[ion(get)]
	pub fn get_redirect(&self) -> String {
		self.redirect.to_string()
	}

	#[ion(get)]
	pub fn get_integrity(&self) -> String {
		self.integrity.clone()
	}

	#[ion(get)]
	pub fn get_keepalive(&self) -> bool {
		self.keepalive
	}

	#[ion(get)]
	pub fn get_is_reload_navigation(&self) -> bool {
		false
	}

	#[ion(get)]
	pub fn get_is_history_navigation(&self) -> bool {
		false
	}

	#[ion(get)]
	pub fn get_signal(&self) -> *mut JSObject {
		self.signal_object.get()
	}

	#[ion(get)]
	pub fn get_duplex(&self) -> String {
		String::from("half")
	}

	#[ion(get)]
	pub fn get_body_used(&self) -> bool {
		self.body.is_none()
	}

	async fn read_to_bytes(&mut self) -> Result<Vec<u8>> {
		if let Some(body) = self.body.take() {
			body.read_to_bytes().await
		} else {
			Err(Error::new("Request body has already been used.", None))
		}
	}

	#[ion(name = "arrayBuffer")]
	pub fn array_buffer<'cx>(&mut self, cx: &'cx Context) -> Option<Promise<'cx>> {
		let this = TracedHeap::new(self.reflector().get());
		future_to_promise::<_, _, _, Error>(cx, |cx| async move {
			let request = Object::from(this.to_local());
			let request = Request::get_mut_private(&cx, &request)?;
			let bytes = request.read_to_bytes().await?;
			Ok(ArrayBufferWrapper::from(bytes))
		})
	}

	pub fn bytes<'cx>(&mut self, cx: &'cx Context) -> Option<Promise<'cx>> {
		let this = TracedHeap::new(self.reflector().get());
		future_to_promise::<_, _, _, Error>(cx, |cx| async move {
			let request = Object::from(this.to_local());
			let request = Request::get_mut_private(&cx, &request)?;
			let bytes = request.read_to_bytes().await?;
			Ok(Uint8ArrayWrapper::from(bytes))
		})
	}

	pub fn text<'cx>(&mut self, cx: &'cx Context) -> Option<Promise<'cx>> {
		let this = TracedHeap::new(self.reflector().get());
		future_to_promise::<_, _, _, Error>(cx, |cx| async move {
			let request = Object::from(this.to_local());
			let request = Request::get_mut_private(&cx, &request)?;
			let bytes = request.read_to_bytes().await?;
			String::from_utf8(bytes).map_err(|e| Error::new(format!("Invalid UTF-8 sequence: {e}"), None))
		})
	}
}

impl Clone for Request {
	fn clone(&self) -> Request {
		let url = self.locations.last().unwrap().clone();

		Request {
			reflector: Reflector::default(),

			headers: Box::default(),
			body: self.body.clone(),

			method: self.method.clone(),
			url: url.clone(),
			locations: vec![url],

			referrer: self.referrer.clone(),
			referrer_policy: self.referrer_policy,

			mode: self.mode,
			credentials: self.credentials,
			cache: self.cache,
			redirect: self.redirect,

			integrity: self.integrity.clone(),

			unsafe_request: true,
			keepalive: self.keepalive,

			client_window: self.client_window,
			signal_object: Heap::boxed(self.signal_object.get()),
		}
	}
}
