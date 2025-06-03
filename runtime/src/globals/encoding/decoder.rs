/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use encoding_rs::{Decoder, DecoderResult, Encoding, UTF_8};
use ion::class::Reflector;
use ion::function::Opt;
use ion::{Error, ErrorKind, FromValue, Result, js_class};

use crate::globals::file::BufferSource;

#[derive(Default, FromValue)]
pub struct TextDecoderOptions {
	#[ion(default)]
	fatal: bool,
	#[ion(default, name = "ignoreBOM")]
	ignore_byte_order_mark: bool,
}

#[derive(Default, FromValue)]
pub struct TextDecodeOptions {
	#[ion(default)]
	stream: bool,
}

#[js_class]
pub struct TextDecoder {
	reflector: Reflector,
	#[trace(no_trace)]
	decoder: Decoder,
	pub fatal: bool,
	pub ignore_byte_order_mark: bool,
}

#[js_class]
impl TextDecoder {
	#[ion(constructor)]
	pub fn constructor(Opt(label): Opt<String>, Opt(options): Opt<TextDecoderOptions>) -> Result<TextDecoder> {
		let encoding;
		if let Some(label) = label {
			let enc = Encoding::for_label_no_replacement(label.as_bytes());
			match enc {
				None => {
					return Err(Error::new(
						format!("The given encoding '{label}' is not supported."),
						ErrorKind::Range,
					));
				}
				Some(enc) => encoding = enc,
			}
		} else {
			encoding = UTF_8;
		}

		let options = options.unwrap_or_default();
		let decoder = if options.ignore_byte_order_mark {
			encoding.new_decoder_without_bom_handling()
		} else {
			encoding.new_decoder()
		};

		Ok(TextDecoder {
			reflector: Reflector::default(),
			decoder,
			fatal: options.fatal,
			ignore_byte_order_mark: options.ignore_byte_order_mark,
		})
	}

	pub fn decode(
		&mut self, #[ion(convert = true)] buffer: BufferSource, Opt(options): Opt<TextDecodeOptions>,
	) -> Result<String> {
		let mut string = String::with_capacity(self.decoder.max_utf8_buffer_length(buffer.len()).unwrap());
		let stream = options.unwrap_or_default().stream;
		if self.fatal {
			let vec_buffer;
			let buffer = if buffer.is_shared() {
				vec_buffer = buffer.to_vec();
				&vec_buffer
			} else {
				unsafe { buffer.as_slice() }
			};

			let (result, _) = self.decoder.decode_to_string_without_replacement(buffer, &mut string, !stream);
			if let DecoderResult::Malformed(_, _) = result {
				return Err(Error::new("TextDecoder.decode: Decoding Failed", ErrorKind::Type));
			}
		} else {
			let (_, _, _) = self.decoder.decode_to_string(unsafe { buffer.as_slice() }, &mut string, !stream);
		}
		Ok(string)
	}

	#[ion(get)]
	pub fn get_encoding(&self) -> String {
		String::from(self.decoder.encoding().name())
	}

	#[ion(get)]
	pub fn get_fatal(&self) -> bool {
		self.fatal
	}

	#[ion(get, name = "ignoreBOM")]
	pub fn get_ignore_bom(&self) -> bool {
		self.ignore_byte_order_mark
	}
}
