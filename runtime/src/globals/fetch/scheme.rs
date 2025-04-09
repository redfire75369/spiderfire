/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::collections::Bound;
use std::iter::once;
use std::str;

use arrayvec::ArrayVec;
use bytes::Bytes;
use data_url::DataUrl;
use headers::{HeaderMapExt, Range};
use http::header::{CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE};
use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use ion::class::Reflector;
use ion::{ClassDefinition, Context, Local, Object};
use tokio::fs::read;
use url::Url;

use crate::ContextExt;
use crate::globals::fetch::header::HeadersKind;
use crate::globals::fetch::response::network_error;
use crate::globals::fetch::{Headers, Request, Response};
use crate::globals::file::Blob;
use crate::globals::url::parse_uuid_from_url_path;

pub async fn scheme_fetch(cx: &Context, scheme: &str, request: &Request, url: Url) -> Response {
	match scheme {
		"about" if url.path() == "blank" => about_blank_fetch(cx, url),
		"blob" => blob_fetch(cx, request, url),
		"data" => data_fetch(cx, url),
		"file" => file_fetch(cx, request, url).await,
		_ => network_error(),
	}
}

fn about_blank_fetch(cx: &Context, url: Url) -> Response {
	let response = Response::new_from_bytes(Bytes::default(), url);
	let headers = Headers {
		reflector: Reflector::default(),
		headers: HeaderMap::from_iter(once((
			CONTENT_TYPE,
			HeaderValue::from_static("text/html;charset=UTF-8"),
		))),
		kind: HeadersKind::Immutable,
	};
	response.headers.set(Headers::new_object(cx, Box::new(headers)));
	response
}

fn blob_fetch(cx: &Context, request: &Request, url: Url) -> Response {
	if request.method != Method::GET {
		return network_error();
	}

	let uuid = match parse_uuid_from_url_path(&url) {
		Some(uuid) => uuid,
		_ => return network_error(),
	};
	let blob = unsafe {
		match cx.get_private().blob_store.get(&uuid) {
			Some(blob) => blob,
			_ => return network_error(),
		}
	};

	let blob = Object::from(unsafe { Local::from_heap(blob) });
	let blob = Blob::get_private(cx, &blob).unwrap();

	let kind = blob.kind.as_deref().unwrap_or("");
	let kind = match HeaderValue::from_str(kind) {
		Ok(kind) => kind,
		Err(_) => return network_error(),
	};

	let mut response_headers = ArrayVec::<_, 3>::new();
	response_headers.push((CONTENT_TYPE, kind));

	let mut bytes = blob.bytes.clone();

	let headers = Object::from(unsafe { Local::from_heap(&request.headers) });
	let headers = &Headers::get_mut_private(cx, &headers).unwrap().headers;

	let (status, range_requested) = match get_ranged_bytes(headers, &mut bytes, &mut response_headers) {
		Ok((status, range_requested)) => (status, range_requested),
		Err(err) => return err,
	};

	response_headers.push((CONTENT_LENGTH, HeaderValue::from(bytes.len())));

	let mut response = Response::new_from_bytes(bytes, url);
	response.status = Some(status);
	response.range_requested = range_requested;

	let headers = Headers {
		reflector: Reflector::default(),
		headers: HeaderMap::from_iter(response_headers),
		kind: HeadersKind::Immutable,
	};
	response.headers.set(Headers::new_object(cx, Box::new(headers)));
	response
}

fn data_fetch(cx: &Context, url: Url) -> Response {
	let data_url = match DataUrl::process(url.as_str()) {
		Ok(data_url) => data_url,
		Err(_) => return network_error(),
	};

	let (body, _) = match data_url.decode_to_vec() {
		Ok(decoded) => decoded,
		Err(_) => return network_error(),
	};
	let mime = data_url.mime_type();
	let mime = format!("{}/{}", mime.type_, mime.subtype);

	let response = Response::new_from_bytes(Bytes::from(body), url);
	let headers = Headers {
		reflector: Reflector::default(),
		headers: HeaderMap::from_iter(once((CONTENT_TYPE, HeaderValue::from_str(&mime).unwrap()))),
		kind: HeadersKind::Immutable,
	};
	response.headers.set(Headers::new_object(cx, Box::new(headers)));
	response
}

async fn file_fetch(cx: &Context, request: &Request, url: Url) -> Response {
	if request.method != Method::GET {
		return network_error();
	}

	match url.to_file_path() {
		Ok(path) => match read(path).await {
			Ok(bytes) => {
				let mut bytes = Bytes::from(bytes);

				let headers = Object::from(unsafe { Local::from_heap(&request.headers) });
				let headers = &Headers::get_mut_private(cx, &headers).unwrap().headers;

				let mut response_headers = ArrayVec::<_, 2>::new();
				let (status, range_requested) = match get_ranged_bytes(headers, &mut bytes, &mut response_headers) {
					Ok((status, range_requested)) => (status, range_requested),
					Err(err) => return err,
				};

				response_headers.push((CONTENT_LENGTH, HeaderValue::from(bytes.len())));

				let mut response = Response::new_from_bytes(bytes, url);
				response.status = Some(status);
				response.range_requested = range_requested;

				let headers = Headers {
					reflector: Reflector::default(),
					headers: HeaderMap::from_iter(response_headers),
					kind: HeadersKind::Immutable,
				};
				response.headers.set(Headers::new_object(cx, Box::new(headers)));

				response
			}
			Err(_) => network_error(),
		},
		Err(_) => network_error(),
	}
}

fn get_ranged_bytes<const N: usize>(
	headers: &HeaderMap, bytes: &mut Bytes, response_headers: &mut ArrayVec<(HeaderName, HeaderValue), N>,
) -> Result<(StatusCode, bool), Response> {
	match headers.typed_try_get::<Range>() {
		Ok(Some(range)) => {
			let len = bytes.len();
			if let Some((start, end)) = range.satisfiable_ranges(len as u64).next() {
				let (start, end) = (start.map(|s| s as usize), end.map(|e| e as usize));
				*bytes = bytes.slice((start, end));

				let (start, end) = match (start, end) {
					(Bound::Included(s), Bound::Included(e)) => (s, e),
					(Bound::Included(s), Bound::Unbounded) => (s, len - 1),
					_ => unreachable!(),
				};
				let range = match HeaderValue::from_str(&format!("{start}-{end}/{len}")) {
					Ok(range) => range,
					Err(_) => return Err(network_error()),
				};

				response_headers.push((CONTENT_RANGE, range));

				Ok((StatusCode::PARTIAL_CONTENT, true))
			} else {
				Err(network_error())
			}
		}
		Ok(None) => Ok((StatusCode::OK, false)),
		Err(_) => Err(network_error()),
	}
}
