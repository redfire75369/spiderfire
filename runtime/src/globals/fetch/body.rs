/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::fmt::{Display, Formatter};
use std::pin::Pin;
use std::task::Poll;
use std::{fmt, task};

use bytes::Bytes;
use form_urlencoded::Serializer;
use http::header::CONTENT_TYPE;
use http::{HeaderMap, HeaderValue};
use http_body_util::{BodyExt, Full};
use hyper::body::{Frame, Incoming, SizeHint};
use ion::conversions::FromValue;
use ion::{Context, Error, ErrorKind, Value};
use mozjs::jsapi::Heap;
use mozjs::jsval::JSVal;
use pin_project::pin_project;

use crate::globals::file::{Blob, BufferSource};
use crate::globals::url::URLSearchParams;

#[derive(Debug, Clone, Traceable)]
#[non_exhaustive]
enum FetchBodyInner {
	None,
	Bytes(#[trace(no_trace)] Bytes),
}

#[derive(Clone, Debug, Traceable)]
#[non_exhaustive]
pub enum FetchBodyKind {
	String,
	Blob(String),
	URLSearchParams,
}

impl Display for FetchBodyKind {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		match self {
			FetchBodyKind::String => f.write_str("text/plain;charset=UTF-8"),
			FetchBodyKind::Blob(mime) => f.write_str(mime),
			FetchBodyKind::URLSearchParams => f.write_str("application/x-www-form-urlencoded;charset=UTF-8"),
		}
	}
}

#[derive(Debug, Traceable)]
pub struct FetchBody {
	body: FetchBodyInner,
	source: Option<Box<Heap<JSVal>>>,
	pub(crate) kind: Option<FetchBodyKind>,
}

impl FetchBody {
	pub fn is_none(&self) -> bool {
		matches!(&self.body, FetchBodyInner::None)
	}

	pub fn is_empty(&self) -> bool {
		match &self.body {
			FetchBodyInner::None => true,
			FetchBodyInner::Bytes(bytes) => bytes.is_empty(),
		}
	}

	pub fn len(&self) -> Option<usize> {
		match &self.body {
			FetchBodyInner::None => None,
			FetchBodyInner::Bytes(bytes) => Some(bytes.len()),
		}
	}

	pub fn is_stream(&self) -> bool {
		!matches!(&self.body, FetchBodyInner::None | FetchBodyInner::Bytes(_))
	}

	pub fn to_http_body(&self) -> Body {
		match &self.body {
			FetchBodyInner::None => Body::Empty,
			FetchBodyInner::Bytes(bytes) => Body::from(bytes.clone()),
		}
	}

	pub async fn read_to_bytes(&self) -> ion::Result<Vec<u8>> {
		Ok(self.to_http_body().collect().await?.to_bytes().to_vec())
	}

	pub(crate) fn add_content_type_header(&self, headers: &mut HeaderMap) {
		if let Some(kind) = &self.kind {
			if !headers.contains_key(CONTENT_TYPE) {
				headers.append(CONTENT_TYPE, HeaderValue::from_str(&kind.to_string()).unwrap());
			}
		}
	}
}

impl Clone for FetchBody {
	fn clone(&self) -> FetchBody {
		FetchBody {
			body: self.body.clone(),
			source: self.source.as_ref().map(|s| Heap::boxed(s.get())),
			kind: self.kind.clone(),
		}
	}
}

impl Default for FetchBody {
	fn default() -> FetchBody {
		FetchBody {
			body: FetchBodyInner::None,
			source: None,
			kind: None,
		}
	}
}

impl<'cx> FromValue<'cx> for FetchBody {
	type Config = ();
	fn from_value(cx: &'cx Context, value: &Value, strict: bool, _: ()) -> ion::Result<FetchBody> {
		if value.handle().is_string() {
			return Ok(FetchBody {
				body: FetchBodyInner::Bytes(Bytes::from(String::from_value(cx, value, strict, ()).unwrap())),
				source: Some(Heap::boxed(value.get())),
				kind: Some(FetchBodyKind::String),
			});
		} else if value.handle().is_object() {
			if let Ok(source) = BufferSource::from_value(cx, value, strict, false) {
				return Ok(FetchBody {
					body: FetchBodyInner::Bytes(source.to_bytes()),
					source: Some(Heap::boxed(value.get())),
					kind: None,
				});
			} else if let Ok(blob) = <&Blob>::from_value(cx, value, strict, ()) {
				return Ok(FetchBody {
					body: FetchBodyInner::Bytes(blob.bytes.clone()),
					source: Some(Heap::boxed(value.get())),
					kind: blob.kind.clone().map(FetchBodyKind::Blob),
				});
			} else if let Ok(search_params) = <&URLSearchParams>::from_value(cx, value, strict, ()) {
				return Ok(FetchBody {
					body: FetchBodyInner::Bytes(Bytes::from(
						Serializer::new(String::new()).extend_pairs(search_params.pairs()).finish(),
					)),
					source: Some(Heap::boxed(value.get())),
					kind: Some(FetchBodyKind::URLSearchParams),
				});
			}
		}
		Err(Error::new("Expected Valid Body", ErrorKind::Type))
	}
}

#[pin_project(project = BodyProject)]
#[derive(Default)]
pub enum Body {
	#[default]
	Empty,
	Once(#[pin] Full<Bytes>),
	Incoming(#[pin] Incoming),
}

impl hyper::body::Body for Body {
	type Data = Bytes;
	type Error = hyper::Error;

	fn poll_frame(
		self: Pin<&mut Self>, cx: &mut task::Context<'_>,
	) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
		match self.project() {
			BodyProject::Empty => Poll::Ready(None),
			BodyProject::Once(full) => full.poll_frame(cx).map_err(|e| match e {}),
			BodyProject::Incoming(incoming) => incoming.poll_frame(cx),
		}
	}

	fn is_end_stream(&self) -> bool {
		match self {
			Body::Empty => true,
			Body::Once(full) => full.is_end_stream(),
			Body::Incoming(incoming) => incoming.is_end_stream(),
		}
	}

	fn size_hint(&self) -> SizeHint {
		match self {
			Body::Empty => SizeHint::with_exact(0),
			Body::Once(full) => full.size_hint(),
			Body::Incoming(incoming) => incoming.size_hint(),
		}
	}
}

impl From<Bytes> for Body {
	fn from(bytes: Bytes) -> Body {
		Body::Once(Full::new(bytes))
	}
}
