/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use encoding_rs::{Encoder, UTF_8};
use ion::class::Reflector;
use ion::function::Opt;
use ion::typedarray::{Uint8Array, Uint8ArrayWrapper};
use ion::{ToValue, js_class};

#[derive(ToValue)]
pub struct EncodeResult {
	read: u64,
	written: u64,
}

#[js_class]
pub struct TextEncoder {
	reflector: Reflector,
	#[trace(no_trace)]
	encoder: Encoder,
}

#[js_class]
impl TextEncoder {
	#[ion(constructor)]
	pub fn constructor() -> TextEncoder {
		TextEncoder {
			reflector: Reflector::default(),
			encoder: UTF_8.new_encoder(),
		}
	}

	pub fn encode(&mut self, Opt(input): Opt<String>) -> Uint8ArrayWrapper {
		let input = input.unwrap_or_default();
		let buf_len = self.encoder.max_buffer_length_from_utf8_if_no_unmappables(input.len()).unwrap();
		let mut buf = Vec::with_capacity(buf_len);
		let (_, _, _) = self.encoder.encode_from_utf8_to_vec(&input, &mut buf, true);
		Uint8ArrayWrapper::from(buf)
	}

	#[ion(name = "encodeInto")]
	pub fn encode_into(&mut self, input: String, destination: Uint8Array) -> EncodeResult {
		let (_, read, written, _) = self.encoder.encode_from_utf8(&input, unsafe { destination.as_mut_slice() }, true);
		EncodeResult {
			read: read as u64,
			written: written as u64,
		}
	}

	#[ion(get)]
	pub fn get_encoding(&self) -> String {
		String::from(self.encoder.encoding().name())
	}
}
