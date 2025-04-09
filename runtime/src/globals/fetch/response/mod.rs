/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use hyper::ext::ReasonPhrase;
use ion::class::{NativeObject, Reflector};
use ion::function::Opt;
use ion::typedarray::{ArrayBufferWrapper, Uint8ArrayWrapper};
use ion::{ClassDefinition, Context, Error, ErrorKind, Object, Promise, Result, TracedHeap};
use mozjs::jsapi::{Heap, JSObject};
pub use options::*;
use url::Url;

use crate::globals::fetch::Headers;
use crate::globals::fetch::body::{Body, FetchBody};
use crate::globals::fetch::header::HeadersKind;
use crate::globals::fetch::response::body::ResponseBody;
use crate::promise::future_to_promise;

mod body;
mod options;

#[js_class]
pub struct Response {
	reflector: Reflector,

	pub(crate) headers: Box<Heap<*mut JSObject>>,
	pub(crate) body: Option<ResponseBody>,

	pub(crate) kind: ResponseKind,
	#[trace(no_trace)]
	pub(crate) url: Option<Url>,
	pub(crate) redirected: bool,

	#[trace(no_trace)]
	pub(crate) status: Option<StatusCode>,
	pub(crate) status_text: Option<String>,

	pub(crate) range_requested: bool,
}

impl Response {
	pub fn from_hyper(response: hyper::Response<Body>, url: Url) -> (HeaderMap, Response) {
		let (parts, body) = response.into_parts();

		let status_text = if let Some(reason) = parts.extensions.get::<ReasonPhrase>() {
			Some(String::from_utf8(reason.as_bytes().to_vec()).unwrap())
		} else {
			parts.status.canonical_reason().map(String::from)
		};

		let response = Response {
			reflector: Reflector::default(),

			headers: Box::default(),
			body: Some(ResponseBody::Hyper(body)),

			kind: ResponseKind::default(),
			url: Some(url),
			redirected: false,

			status: Some(parts.status),
			status_text,

			range_requested: false,
		};

		(parts.headers, response)
	}

	pub fn new_from_bytes(bytes: Bytes, url: Url) -> Response {
		Response {
			reflector: Reflector::default(),

			headers: Box::default(),
			body: Some(ResponseBody::Hyper(Body::from(bytes))),

			kind: ResponseKind::Basic,
			url: Some(url),
			redirected: false,

			status: Some(StatusCode::OK),
			status_text: Some(String::from("OK")),

			range_requested: false,
		}
	}
}

#[js_class]
impl Response {
	#[ion(constructor)]
	pub fn constructor(cx: &Context, Opt(body): Opt<FetchBody>, Opt(init): Opt<ResponseInit>) -> Result<Response> {
		let init = init.unwrap_or_default();

		let mut response = Response {
			reflector: Reflector::default(),

			headers: Box::default(),
			body: Some(ResponseBody::Hyper(Body::Empty)),

			kind: ResponseKind::default(),
			url: None,
			redirected: false,

			status: Some(init.status),
			status_text: init.status_text,

			range_requested: false,
		};

		let mut headers = init.headers.into_headers(HeaderMap::new(), HeadersKind::Response)?;

		if let Some(body) = body {
			if init.status == StatusCode::NO_CONTENT
				|| init.status == StatusCode::RESET_CONTENT
				|| init.status == StatusCode::NOT_MODIFIED
			{
				return Err(Error::new(
					"Received non-null body with null body status.",
					ErrorKind::Type,
				));
			}

			body.add_content_type_header(&mut headers.headers);
			response.body = Some(ResponseBody::Fetch(body));
		}

		response.headers.set(Headers::new_object(cx, Box::new(headers)));

		Ok(response)
	}

	#[ion(get)]
	pub fn get_type(&self) -> String {
		self.kind.to_string()
	}

	#[ion(get)]
	pub fn get_url(&self) -> String {
		self.url.as_ref().map(Url::to_string).unwrap_or_default()
	}

	#[ion(get)]
	pub fn get_redirected(&self) -> bool {
		self.redirected
	}

	#[ion(get)]
	pub fn get_status(&self) -> u16 {
		self.status.as_ref().map(StatusCode::as_u16).unwrap_or_default()
	}

	#[ion(get)]
	pub fn get_ok(&self) -> bool {
		self.status.as_ref().map(StatusCode::is_success).unwrap_or_default()
	}

	#[ion(get)]
	pub fn get_status_text(&self) -> String {
		self.status_text.clone().unwrap_or_default()
	}

	#[ion(get)]
	pub fn get_headers(&self) -> *mut JSObject {
		self.headers.get()
	}

	#[ion(get)]
	pub fn get_body_used(&self) -> bool {
		self.body.is_none()
	}

	async fn read_to_bytes(&mut self) -> Result<Vec<u8>> {
		if let Some(body) = self.body.take() {
			body.read_to_bytes().await
		} else {
			Err(Error::new("Response body has already been used.", None))
		}
	}

	#[ion(name = "arrayBuffer")]
	pub fn array_buffer<'cx>(&mut self, cx: &'cx Context) -> Option<Promise<'cx>> {
		let this = TracedHeap::new(self.reflector().get());
		future_to_promise::<_, _, _, Error>(cx, |cx| async move {
			let response = Object::from(this.to_local());
			let response = Response::get_mut_private(&cx, &response)?;
			let bytes = response.read_to_bytes().await?;
			Ok(ArrayBufferWrapper::from(bytes))
		})
	}

	pub fn bytes<'cx>(&mut self, cx: &'cx Context) -> Option<Promise<'cx>> {
		let this = TracedHeap::new(self.reflector().get());
		future_to_promise::<_, _, _, Error>(cx, |cx| async move {
			let response = Object::from(this.to_local());
			let response = Response::get_mut_private(&cx, &response)?;
			let bytes = response.read_to_bytes().await?;
			Ok(Uint8ArrayWrapper::from(bytes))
		})
	}

	pub fn text<'cx>(&mut self, cx: &'cx Context) -> Option<Promise<'cx>> {
		let this = TracedHeap::new(self.reflector().get());
		future_to_promise::<_, _, _, Error>(cx, |cx| async move {
			let response = Object::from(this.to_local());
			let response = Response::get_mut_private(&cx, &response)?;
			let bytes = response.read_to_bytes().await?;
			String::from_utf8(bytes).map_err(|e| Error::new(format!("Invalid UTF-8 sequence: {e}"), None))
		})
	}
}

pub fn network_error() -> Response {
	Response {
		reflector: Reflector::default(),

		headers: Box::default(),
		body: None,

		kind: ResponseKind::Error,
		url: None,
		redirected: false,

		status: None,
		status_text: None,

		range_requested: false,
	}
}
