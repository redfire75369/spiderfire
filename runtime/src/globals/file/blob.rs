/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::str::FromStr;

use bytes::{BufMut, Bytes, BytesMut};
use encoding_rs::UTF_8;
use ion::class::Reflector;
use ion::conversions::FromValue;
use ion::format::NEWLINE;
use ion::function::{Clamp, Opt};
use ion::typedarray::{ArrayBuffer, ArrayBufferView, ArrayBufferWrapper, Uint8ArrayWrapper};
use ion::{ClassDefinition, Context, Error, ErrorKind, Object, Promise, Result, Value};
use mozjs::jsapi::JSObject;

use crate::promise::future_to_promise;

#[derive(Debug)]
pub enum BufferSource<'cx> {
	Buffer(ArrayBuffer<'cx>),
	View(ArrayBufferView<'cx>),
}

impl BufferSource<'_> {
	pub fn len(&self) -> usize {
		unsafe { self.as_slice().len() }
	}

	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}

	pub fn is_shared(&self) -> bool {
		match self {
			BufferSource::Buffer(buffer) => buffer.is_shared(),
			BufferSource::View(view) => view.is_shared(),
		}
	}

	pub unsafe fn as_slice(&self) -> &[u8] {
		match self {
			BufferSource::Buffer(buffer) => unsafe { buffer.as_slice() },
			BufferSource::View(view) => unsafe { view.as_slice() },
		}
	}

	pub fn to_vec(&self) -> Vec<u8> {
		unsafe { self.as_slice().to_vec() }
	}

	pub fn to_bytes(&self) -> Bytes {
		Bytes::copy_from_slice(unsafe { self.as_slice() })
	}
}

impl<'cx> FromValue<'cx> for BufferSource<'cx> {
	type Config = bool;
	fn from_value(cx: &'cx Context, value: &Value, strict: bool, allow_shared: bool) -> Result<BufferSource<'cx>> {
		let obj = Object::from_value(cx, value, strict, ())?;
		if let Some(buffer) = ArrayBuffer::from(cx.root(obj.handle().get())) {
			if buffer.is_shared() && !allow_shared {
				return Err(Error::new("Buffer Source cannot be shared", ErrorKind::Type));
			}
			Ok(BufferSource::Buffer(buffer))
		} else if let Some(view) = ArrayBufferView::from(obj.into_local()) {
			if view.is_shared() && !allow_shared {
				return Err(Error::new("Buffer Source cannot be shared", ErrorKind::Type));
			}
			Ok(BufferSource::View(view))
		} else {
			Err(Error::new("Object is not a buffer source.", ErrorKind::Type))
		}
	}
}

#[derive(Debug, FromValue)]
#[ion(inherit)]
pub enum BlobPart<'cx> {
	String(String),
	BufferSource(#[ion(convert = false)] BufferSource<'cx>),
	Blob(&'cx Blob),
}

#[derive(Clone, Copy, Debug, Default)]
pub enum Endings {
	#[default]
	Transparent,
	Native,
}

impl Endings {
	fn convert(self, buffer: &mut BytesMut, string: &str) {
		let string = string.as_bytes();
		match self {
			Endings::Transparent => buffer.extend(string),
			Endings::Native => {
				let mut i = 0;
				while let Some(&b) = string.get(i) {
					i += 1;
					if b == b'\r' {
						buffer.extend_from_slice(NEWLINE.as_bytes());
						if string.get(i) == Some(&b'\n') {
							i += 1;
						}
					} else if b == b'\n' {
						buffer.extend_from_slice(NEWLINE.as_bytes());
					} else {
						buffer.put_u8(b);
					}
				}
			}
		}
	}
}

impl FromStr for Endings {
	type Err = Error;

	fn from_str(endings: &str) -> Result<Endings> {
		match endings {
			"transparent" => Ok(Endings::Transparent),
			"native" => Ok(Endings::Native),
			_ => Err(Error::new("Invalid ending type for Blob", ErrorKind::Type)),
		}
	}
}

impl<'cx> FromValue<'cx> for Endings {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, strict: bool, _: ()) -> Result<Endings> {
		let endings = String::from_value(cx, value, strict, ())?;
		Endings::from_str(&endings)
	}
}

#[derive(Debug, Default, FromValue)]
pub struct BlobOptions {
	#[ion(name = "type")]
	kind: Option<String>,
	#[ion(default)]
	endings: Endings,
}

fn validate_kind(kind: Option<String>) -> Option<String> {
	kind.filter(|kind| kind.as_bytes().iter().all(|b| matches!(b, 0x20..=0x7E)))
		.map(|mut kind| {
			kind.make_ascii_lowercase();
			kind
		})
}

#[derive(Debug)]
#[js_class]
pub struct Blob {
	pub(crate) reflector: Reflector,
	#[trace(no_trace)]
	pub(crate) bytes: Bytes,
	pub(crate) kind: Option<String>,
}

#[js_class]
impl Blob {
	#[ion(constructor)]
	pub fn constructor(Opt(parts): Opt<Vec<BlobPart>>, Opt(options): Opt<BlobOptions>) -> Blob {
		let options = options.unwrap_or_default();

		let mut bytes = BytesMut::new();

		if let Some(parts) = parts {
			for part in parts {
				match part {
					BlobPart::String(str) => options.endings.convert(&mut bytes, &str),
					BlobPart::BufferSource(source) => bytes.extend_from_slice(unsafe { source.as_slice() }),
					BlobPart::Blob(blob) => bytes.extend_from_slice(&blob.bytes),
				}
			}
		}

		Blob {
			reflector: Reflector::default(),
			bytes: bytes.freeze(),
			kind: validate_kind(options.kind),
		}
	}

	#[ion(get)]
	pub fn get_size(&self) -> u64 {
		self.bytes.len() as u64
	}

	#[ion(get)]
	pub fn get_type(&self) -> String {
		self.kind.clone().unwrap_or_default()
	}

	pub fn slice(
		&self, cx: &Context, Opt(start): Opt<Clamp<i64>>, Opt(end): Opt<Clamp<i64>>, Opt(kind): Opt<String>,
	) -> *mut JSObject {
		let size = self.bytes.len() as i64;

		let mut start = start.unwrap_or_default().0;
		if start < 0 {
			start = 0.max(size + start);
		}
		let start = start.min(size) as usize;

		let mut end = end.unwrap_or(Clamp(size)).0;
		if end < 0 {
			end = 0.max(size + end);
		}
		let end = end.min(size) as usize;

		let span = 0.max(end - start);
		let bytes = self.bytes.slice(start..start + span);

		let blob = Blob {
			reflector: Reflector::default(),
			bytes,
			kind: validate_kind(kind),
		};
		Blob::new_object(cx, Box::new(blob))
	}

	pub fn text<'cx>(&self, cx: &'cx Context) -> Option<Promise<'cx>> {
		let bytes = self.bytes.clone();
		future_to_promise(cx, |_| async move { Ok::<_, ()>(UTF_8.decode(&bytes).0.into_owned()) })
	}

	#[ion(name = "arrayBuffer")]
	pub fn array_buffer<'cx>(&self, cx: &'cx Context) -> Option<Promise<'cx>> {
		let bytes = self.bytes.clone();
		future_to_promise(
			cx,
			|_| async move { Ok::<_, ()>(ArrayBufferWrapper::from(bytes.to_vec())) },
		)
	}

	pub fn bytes<'cx>(&self, cx: &'cx Context) -> Option<Promise<'cx>> {
		let bytes = self.bytes.clone();
		future_to_promise(
			cx,
			|_| async move { Ok::<_, ()>(Uint8ArrayWrapper::from(bytes.to_vec())) },
		)
	}
}
