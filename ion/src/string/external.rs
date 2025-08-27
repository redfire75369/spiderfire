/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::ffi::c_void;

use mozjs::glue::{CreateJSExternalStringCallbacks, JSExternalStringCallbacksTraps};
use mozjs::jsapi::JSExternalStringCallbacks;

mod latin1 {
	use std::ffi::c_void;

	use mozjs::jsapi::MallocSizeOf;

	use crate::utils::BoxExt as _;

	pub(crate) unsafe extern "C" fn finalise(data: *const c_void, chars: *mut u8) {
		let _ = unsafe { Box::from_raw_parts(chars, data as usize) };
	}

	pub(crate) extern "C" fn size_of(data: *const c_void, _: *const u8, _: MallocSizeOf) -> usize {
		data as usize
	}
}

mod utf16 {
	use std::ffi::c_void;

	use mozjs::jsapi::MallocSizeOf;

	use crate::utils::BoxExt as _;

	pub(crate) unsafe extern "C" fn finalise(data: *const c_void, chars: *mut u16) {
		let _ = unsafe { Box::from_raw_parts(chars.cast::<u8>(), data as usize * 2) };
	}

	pub(crate) extern "C" fn size_of(data: *const c_void, _: *const u16, _: MallocSizeOf) -> usize {
		data as usize
	}
}

static EXTERNAL_STRING_CALLBACKS_TRAPS: JSExternalStringCallbacksTraps = JSExternalStringCallbacksTraps {
	latin1Finalize: Some(latin1::finalise),
	latin1SizeOfBuffer: Some(latin1::size_of),
	utf16Finalize: Some(utf16::finalise),
	utf16SizeOfBuffer: Some(utf16::size_of),
};

pub(super) fn create_callbacks(len: usize) -> *mut JSExternalStringCallbacks {
	unsafe { CreateJSExternalStringCallbacks(&raw const EXTERNAL_STRING_CALLBACKS_TRAPS, len as *mut c_void) }
}
