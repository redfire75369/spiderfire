/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::ops::{Deref, DerefMut};

use mozjs::jsapi::{
	IsMapObject, JSObject, MapClear, MapDelete, MapEntries, MapForEach, MapGet, MapHas, MapKeys, MapSet, MapSize,
	MapValues, NewMapObject,
};

use crate::conversions::ToValue as _;
use crate::{Context, Function, Local, Object, Value};

pub struct Map<'m> {
	map: Local<'m, *mut JSObject>,
}

impl<'m> Map<'m> {
	/// Creates a new empty [Map].
	pub fn new(cx: &'m Context) -> Map<'m> {
		Map {
			map: cx.root(unsafe { NewMapObject(cx.as_ptr()) }),
		}
	}

	/// Creates a [Map] from an [Object].
	///
	/// Returns [None] if the object is not a map.
	pub fn from(cx: &Context, object: Local<'m, *mut JSObject>) -> Option<Map<'m>> {
		Map::is_map(cx, &object).then_some(Map { map: object })
	}

	/// Creates a [Map] from an [Object].
	///
	/// ### Safety
	/// Object must be a map.
	pub unsafe fn from_unchecked(object: Local<'m, *mut JSObject>) -> Map<'m> {
		Map { map: object }
	}

	/// Returns the size of the [Map].
	pub fn size(&self, cx: &Context) -> u32 {
		unsafe { MapSize(cx.as_ptr(), self.handle().into()) }
	}

	/// Checks if the [Map] contains the given key.
	pub fn has(&self, cx: &Context, key: &Value) -> bool {
		let mut has = false;
		unsafe { MapHas(cx.as_ptr(), self.handle().into(), key.handle().into(), &raw mut has) && has }
	}

	/// Returns the value of the [Map] at the given key.
	pub fn get<'cx>(&self, cx: &'cx Context, key: &Value) -> Option<Value<'cx>> {
		self.has(cx, key).then(|| {
			let mut rval = Value::undefined(cx);
			unsafe {
				MapGet(
					cx.as_ptr(),
					self.handle().into(),
					key.handle().into(),
					rval.handle_mut().into(),
				);
			}
			rval
		})
	}

	/// Sets the value of the [Map] at the given key.
	pub fn set(&self, cx: &Context, key: &Value, value: &Value) -> bool {
		unsafe {
			MapSet(
				cx.as_ptr(),
				self.handle().into(),
				key.handle().into(),
				value.handle().into(),
			)
		}
	}

	/// Deletes the value of the [Map] at the given key.
	pub fn delete(&self, cx: &Context, key: &Value) -> bool {
		let mut rval = false;
		unsafe { MapDelete(cx.as_ptr(), self.handle().into(), key.handle().into(), &raw mut rval) && rval }
	}

	/// Clears the contents of the [Map].
	pub fn clear(&self, cx: &Context) -> bool {
		unsafe { MapClear(cx.as_ptr(), self.handle().into()) }
	}

	/// Returns an Iterator over the keys of the [Map].
	pub fn keys<'cx>(&self, cx: &'cx Context) -> Object<'cx> {
		let mut keys = Value::undefined(cx);
		unsafe {
			MapKeys(cx.as_ptr(), self.handle().into(), keys.handle_mut().into());
		}
		keys.to_object(cx)
	}

	/// Returns an Iterator over the values of the [Map].
	pub fn values<'cx>(&self, cx: &'cx Context) -> Object<'cx> {
		let mut values = Value::undefined(cx);
		unsafe {
			MapValues(cx.as_ptr(), self.handle().into(), values.handle_mut().into());
		}
		values.to_object(cx)
	}

	/// Returns an Iterator over the entries of the [Map].
	pub fn entries<'cx>(&self, cx: &'cx Context) -> Object<'cx> {
		let mut entries = Value::undefined(cx);
		unsafe {
			MapEntries(cx.as_ptr(), self.handle().into(), entries.handle_mut().into());
		}
		entries.to_object(cx)
	}

	/// Runs the given callback for each entry in the [Map].
	pub fn for_each(&self, cx: &Context, callback: &Function, this: &Object) -> bool {
		unsafe {
			MapForEach(
				cx.as_ptr(),
				self.handle().into(),
				callback.as_value(cx).handle().into(),
				this.as_value(cx).handle().into(),
			)
		}
	}

	/// Checks if the object is a map.
	pub fn is_map(cx: &Context, object: &Local<*mut JSObject>) -> bool {
		let mut map = false;
		unsafe { IsMapObject(cx.as_ptr(), object.handle().into(), &raw mut map) && map }
	}
}

impl<'m> Deref for Map<'m> {
	type Target = Local<'m, *mut JSObject>;

	fn deref(&self) -> &Self::Target {
		&self.map
	}
}

impl DerefMut for Map<'_> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.map
	}
}
