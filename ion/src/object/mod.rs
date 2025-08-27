/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::ptr;

pub use array::Array;
pub use date::Date;
pub use descriptor::PropertyDescriptor;
pub use iterator::{Iterator, JSIterator};
pub use key::{OwnedKey, PropertyKey};
pub use map::Map;
use mozjs::jsapi::{
	JS_NewGlobalObject, JSCLASS_RESERVED_SLOTS_MASK, JSCLASS_RESERVED_SLOTS_SHIFT, JSClass, JSPrincipals,
	OnNewGlobalHookOption,
};
use mozjs::rust::{RealmOptions, SIMPLE_GLOBAL_CLASS};
pub use object::Object;
pub use promise::Promise;
pub use regexp::RegExp;
pub use set::Set;

use crate::Context;

mod array;
mod date;
mod descriptor;
mod iterator;
mod key;
mod map;
mod object;
mod promise;
mod regexp;
mod set;
pub mod typedarray;

/// Returns the bit-masked representation of reserved slots for a class.
pub const fn class_reserved_slots(slots: u32) -> u32 {
	(slots & JSCLASS_RESERVED_SLOTS_MASK) << JSCLASS_RESERVED_SLOTS_SHIFT
}

pub const unsafe fn class_num_reserved_slots(class: *const JSClass) -> u32 {
	unsafe { ((*class).flags >> JSCLASS_RESERVED_SLOTS_SHIFT) & JSCLASS_RESERVED_SLOTS_MASK }
}

pub fn new_global<'cx>(
	cx: &'cx Context, class: &JSClass, principals: Option<*mut JSPrincipals>, hook_option: OnNewGlobalHookOption,
	realm_options: Option<RealmOptions>,
) -> Object<'cx> {
	let realm_options = realm_options.unwrap_or_default();
	let global = unsafe {
		JS_NewGlobalObject(
			cx.as_ptr(),
			class,
			principals.unwrap_or_else(ptr::null_mut),
			hook_option,
			&raw const *realm_options,
		)
	};
	Object::from(cx.root(global))
}

pub fn default_new_global(cx: &Context) -> Object<'_> {
	let mut options = RealmOptions::default();
	options.creationOptions_.sharedMemoryAndAtomics_ = true;
	options.creationOptions_.defineSharedArrayBufferConstructor_ = true;

	new_global(
		cx,
		&SIMPLE_GLOBAL_CLASS,
		None,
		OnNewGlobalHookOption::FireOnNewGlobalHook,
		Some(options),
	)
}
