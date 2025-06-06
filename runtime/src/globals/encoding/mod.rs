/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

pub use decoder::TextDecoder;
pub use encoder::TextEncoder;
use ion::{ClassDefinition as _, Context, Object};

mod decoder;
mod encoder;

pub fn define(cx: &Context, global: &Object) -> bool {
	TextDecoder::init_class(cx, global).0 && TextEncoder::init_class(cx, global).0
}
