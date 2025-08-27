/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::ops::{Deref, DerefMut};

use mozjs::jsapi::{
	IsSetObject, JSObject, NewSetObject, SetAdd, SetClear, SetDelete, SetEntries, SetForEach, SetHas, SetKeys, SetSize,
};

use crate::conversions::ToValue as _;
use crate::{Context, Function, Local, Object, Value};

pub struct Set<'s> {
	set: Local<'s, *mut JSObject>,
}

impl<'s> Set<'s> {
	/// Creates a new empty [Set].
	pub fn new(cx: &'s Context) -> Set<'s> {
		Set {
			set: cx.root(unsafe { NewSetObject(cx.as_ptr()) }),
		}
	}

	/// Creates a [Set] from an [Object].
	///
	/// Returns [None] if the object is not a set.
	pub fn from(cx: &Context, object: Local<'s, *mut JSObject>) -> Option<Set<'s>> {
		Set::is_set(cx, &object).then_some(Set { set: object })
	}

	/// Creates a [Set] from an [Object].
	///
	/// ### Safety
	/// Object must be a set.
	pub unsafe fn from_unchecked(object: Local<'s, *mut JSObject>) -> Set<'s> {
		Set { set: object }
	}

	/// Returns the size of the [Set].
	pub fn size(&self, cx: &Context) -> u32 {
		unsafe { SetSize(cx.as_ptr(), self.handle().into()) }
	}

	/// Checks if the [Set] contains the given key.
	pub fn has(&self, cx: &Context, key: &Value) -> bool {
		let mut has = false;
		unsafe { SetHas(cx.as_ptr(), self.handle().into(), key.handle().into(), &raw mut has) && has }
	}

	/// Adds the key to the [Set].
	pub fn add(&self, cx: &Context, key: &Value) -> bool {
		unsafe { SetAdd(cx.as_ptr(), self.handle().into(), key.handle().into()) }
	}

	/// Deletes the key from the [Set].
	pub fn delete(&self, cx: &Context, key: &Value) -> bool {
		let mut rval = false;
		unsafe { SetDelete(cx.as_ptr(), self.handle().into(), key.handle().into(), &raw mut rval) && rval }
	}

	/// Clears the contents of the [Set].
	pub fn clear(&self, cx: &Context) -> bool {
		unsafe { SetClear(cx.as_ptr(), self.handle().into()) }
	}

	/// Returns an Iterator over the keys of the [Set].
	pub fn keys<'cx>(&self, cx: &'cx Context) -> Object<'cx> {
		let mut keys = Value::undefined(cx);
		unsafe {
			SetKeys(cx.as_ptr(), self.handle().into(), keys.handle_mut().into());
		}
		keys.to_object(cx)
	}

	/// Returns an Iterator over the entries of the [Set].
	/// The key and value in each entry are the same.
	pub fn entries<'cx>(&self, cx: &'cx Context) -> Object<'cx> {
		let mut entries = Value::undefined(cx);
		unsafe {
			SetEntries(cx.as_ptr(), self.handle().into(), entries.handle_mut().into());
		}
		entries.to_object(cx)
	}

	/// Runs the given callback for each entry in the [Set].
	pub fn for_each(&self, cx: &Context, callback: &Function, this: &Object) -> bool {
		unsafe {
			SetForEach(
				cx.as_ptr(),
				self.handle().into(),
				callback.as_value(cx).handle().into(),
				this.as_value(cx).handle().into(),
			)
		}
	}

	/// Checks if the object is a set.
	pub fn is_set(cx: &Context, object: &Local<*mut JSObject>) -> bool {
		let mut set = false;
		unsafe { IsSetObject(cx.as_ptr(), object.handle().into(), &raw mut set) && set }
	}
}

impl<'s> Deref for Set<'s> {
	type Target = Local<'s, *mut JSObject>;

	fn deref(&self) -> &Self::Target {
		&self.set
	}
}

impl DerefMut for Set<'_> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.set
	}
}
