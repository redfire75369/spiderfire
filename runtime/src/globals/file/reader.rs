/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::cell::UnsafeCell;
use std::str::FromStr;

use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use encoding_rs::{Encoding, UTF_8};
use ion::class::{NativeObject, Reflector};
use ion::conversions::ToValue;
use ion::function::Opt;
use ion::string::byte::{ByteString, Latin1};
use ion::typedarray::ArrayBufferWrapper;
use ion::{ClassDefinition, Context, Error, ErrorKind, Object, Result, TracedHeap};
use mime::Mime;
use mozjs::jsapi::{Heap, JSObject};
use mozjs::jsval::{JSVal, NullValue};

use crate::globals::file::Blob;
use crate::promise::future_to_promise;

fn encoding_from_string_mime(encoding: Option<&str>, mime: Option<&str>) -> &'static Encoding {
	encoding
		.and_then(|e| match Encoding::for_label_no_replacement(e.as_bytes()) {
			None if mime.is_some() => Mime::from_str(mime.unwrap()).ok().and_then(|mime| {
				Encoding::for_label_no_replacement(
					mime.get_param("charset").map(|p| p.as_str().as_bytes()).unwrap_or(b""),
				)
			}),
			e => e,
		})
		.unwrap_or(UTF_8)
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Traceable)]
#[repr(u8)]
pub enum FileReaderState {
	#[default]
	Empty = 0,
	Loading = 1,
	Done = 2,
}

impl FileReaderState {
	fn validate(&mut self) -> Result<()> {
		if FileReaderState::Loading == *self {
			return Err(Error::new("Invalid State for File Reader", ErrorKind::Type));
		}
		*self = FileReaderState::Loading;
		Ok(())
	}
}

#[derive(Debug)]
#[js_class]
pub struct FileReader {
	reflector: Reflector,
	state: FileReaderState,
	result: Heap<JSVal>,
	error: Heap<*mut JSObject>,
}

#[js_class]
impl FileReader {
	pub const EMPTY: i32 = FileReaderState::Empty as u8 as i32;
	pub const LOADING: i32 = FileReaderState::Loading as u8 as i32;
	pub const DONE: i32 = FileReaderState::Done as u8 as i32;

	#[ion(constructor)]
	pub fn constructor() -> FileReader {
		FileReader::default()
	}

	#[ion(get)]
	pub fn get_ready_state(&self) -> u8 {
		self.state as u8
	}

	#[ion(get)]
	pub fn get_result(&self) -> JSVal {
		self.result.get()
	}

	#[ion(get)]
	pub fn get_error(&self) -> *mut JSObject {
		self.error.get()
	}

	#[ion(name = "readAsArrayBuffer")]
	pub fn read_as_array_buffer(&mut self, cx: &Context, blob: &Blob) -> Result<()> {
		self.state.validate()?;
		let bytes = blob.bytes.clone();

		let this = TracedHeap::new(self.reflector().get());

		future_to_promise::<_, _, _, Error>(cx, |cx| async move {
			let reader = Object::from(this.to_local());
			let reader = FileReader::get_private(&cx, &reader)?;
			let array_buffer = ArrayBufferWrapper::from(bytes.to_vec());
			reader.result.set(array_buffer.as_value(&cx).get());
			Ok(())
		});
		Ok(())
	}

	#[ion(name = "readAsBinaryString")]
	pub fn read_as_binary_string(&mut self, cx: &Context, blob: &Blob) -> Result<()> {
		self.state.validate()?;
		let bytes = blob.bytes.clone();

		let this = TracedHeap::new(self.reflector().get());

		future_to_promise::<_, _, _, Error>(cx, |cx| async move {
			let reader = Object::from(this.to_local());
			let reader = FileReader::get_private(&cx, &reader)?;
			let byte_string = unsafe { ByteString::<Latin1>::from_unchecked(bytes.to_vec()) };
			reader.result.set(byte_string.as_value(&cx).get());
			Ok(())
		});
		Ok(())
	}

	#[ion(name = "readAsText")]
	pub fn read_as_text(&mut self, cx: &Context, blob: &Blob, Opt(encoding): Opt<String>) -> Result<()> {
		self.state.validate()?;
		let bytes = blob.bytes.clone();
		let mime = blob.kind.clone();

		let this = TracedHeap::new(self.reflector().get());

		future_to_promise::<_, _, _, Error>(cx, |cx| async move {
			let encoding = encoding_from_string_mime(encoding.as_deref(), mime.as_deref());

			let reader = Object::from(this.to_local());
			let reader = FileReader::get_private(&cx, &reader)?;
			let str = encoding.decode_without_bom_handling(&bytes).0;
			reader.result.set(str.as_value(&cx).get());
			Ok(())
		});
		Ok(())
	}

	#[ion(name = "readAsDataURL")]
	pub fn read_as_data_url(&mut self, cx: &Context, blob: &Blob) -> Result<()> {
		self.state.validate()?;
		let bytes = blob.bytes.clone();
		let mime = blob.kind.clone();

		let this = TracedHeap::new(self.reflector().get());

		future_to_promise::<_, _, _, Error>(cx, |cx| async move {
			let reader = Object::from(this.to_local());
			let reader = FileReader::get_private(&cx, &reader)?;
			let base64 = BASE64_STANDARD.encode(&bytes);
			let data_url = match mime {
				Some(mime) => format!("data:{mime};base64,{base64}"),
				None => format!("data:base64,{base64}"),
			};

			reader.result.set(data_url.as_value(&cx).get());
			Ok(())
		});
		Ok(())
	}
}

impl Default for FileReader {
	fn default() -> FileReader {
		FileReader {
			reflector: Reflector::default(),
			state: FileReaderState::default(),
			result: Heap { ptr: UnsafeCell::from(NullValue()) },
			error: Heap::default(),
		}
	}
}

#[derive(Debug, Default)]
#[js_class]
pub struct FileReaderSync {
	reflector: Reflector,
}

#[js_class]
impl FileReaderSync {
	#[ion(constructor)]
	pub fn constructor() -> FileReaderSync {
		FileReaderSync::default()
	}

	#[ion(name = "readAsArrayBuffer")]
	pub fn read_as_array_buffer(&mut self, blob: &Blob) -> ArrayBufferWrapper {
		ArrayBufferWrapper::from(blob.bytes.to_vec())
	}

	#[ion(name = "readAsBinaryString")]
	pub fn read_as_binary_string(&mut self, blob: &Blob) -> ByteString {
		unsafe { ByteString::<Latin1>::from_unchecked(blob.bytes.to_vec()) }
	}

	#[ion(name = "readAsText")]
	pub fn read_as_text(&mut self, blob: &Blob, Opt(encoding): Opt<String>) -> String {
		let encoding = encoding_from_string_mime(encoding.as_deref(), blob.kind.as_deref());
		encoding.decode_without_bom_handling(&blob.bytes).0.into_owned()
	}

	#[ion(name = "readAsDataURL")]
	pub fn read_as_data_url(&mut self, blob: &Blob) -> String {
		let mime = blob.kind.clone();

		let base64 = BASE64_STANDARD.encode(&blob.bytes);
		match mime {
			Some(mime) => format!("data:{mime};base64,{base64}"),
			None => format!("data:base64,{base64}"),
		}
	}
}
