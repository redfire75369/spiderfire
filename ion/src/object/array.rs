/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::iter::FusedIterator;
use std::ops::{Deref, DerefMut};

use mozjs::gc::RootedVec;
use mozjs::jsapi::{GetArrayLength, HandleValueArray, IsArray, JSObject, NewArrayObject, NewArrayObject1};
use mozjs::jsval::{JSVal, ObjectValue};

use crate::conversions::{FromValue, ToValue};
use crate::flags::{IteratorFlags, PropertyFlags};
use crate::object::object::ObjectKeysIter;
use crate::{Context, Local, Object, PropertyDescriptor, Result, Value};

/// Represents an [Array] in the JavaScript Runtime.
/// Refer to [MDN](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Array) for more details.
#[derive(Debug)]
pub struct Array<'a> {
	arr: Object<'a>,
}

impl<'a> Array<'a> {
	/// Creates an empty [Array].
	pub fn new(cx: &'a Context) -> Array<'a> {
		Array::new_with_length(cx, 0)
	}

	/// Creates an empty [Array] with the given length.
	pub fn new_with_length(cx: &'a Context, length: usize) -> Array<'a> {
		Array {
			arr: cx.root(unsafe { NewArrayObject1(cx.as_ptr(), length) }).into(),
		}
	}

	/// Creates an [Array] from a slice of values.
	pub fn from_rooted_vec(cx: &'a Context, vec: &RootedVec<'_, JSVal>) -> Array<'a> {
		Array::from_handle(cx, HandleValueArray::from(vec))
	}

	/// Creates an [Array] from a [HandleValueArray].
	pub fn from_handle(cx: &'a Context, handle: HandleValueArray) -> Array<'a> {
		Array {
			arr: cx.root(unsafe { NewArrayObject(cx.as_ptr(), &handle) }).into(),
		}
	}

	/// Creates an [Array] from an object.
	///
	/// Returns [None] if the object is not an array.
	pub fn from(cx: &Context, object: Local<'a, *mut JSObject>) -> Option<Array<'a>> {
		if Array::is_array(cx, &object) {
			Some(Array { arr: object.into() })
		} else {
			None
		}
	}

	/// Creates an [Array] from an object.
	///
	/// ### Safety
	/// Object must be an array.
	pub unsafe fn from_unchecked(object: Local<'a, *mut JSObject>) -> Array<'a> {
		Array { arr: object.into() }
	}

	/// Converts an [Array] to a [Vec].
	/// Returns an empty [Vec] if the conversion fails.
	pub fn to_vec<'cx>(&self, cx: &'cx Context) -> Vec<Value<'cx>> {
		let value = cx.root(ObjectValue(self.arr.handle().get())).into();
		Vec::from_value(cx, &value, true, ()).unwrap_or_default()
	}

	/// Converts an [Array] to an [Object].
	pub fn to_object(&self, cx: &'a Context) -> Object<'a> {
		Object::from(cx.root(self.arr.handle().get()))
	}

	/// Returns the length of the [Array].
	pub fn len(&self, cx: &Context) -> u32 {
		let mut length = 0;
		unsafe {
			GetArrayLength(cx.as_ptr(), self.handle().into(), &mut length);
		}
		length
	}

	/// Returns if the [Array] is empty.
	pub fn is_empty(&self, cx: &Context) -> bool {
		self.len(cx) == 0
	}

	/// Checks if the [Array] has a value at the given index.
	pub fn has(&self, cx: &Context, index: u32) -> bool {
		self.arr.has(cx, index)
	}

	/// Gets the [Value] at the given index of the [Array].
	/// Returns [None] if there is no value at the given index.
	pub fn get<'cx>(&self, cx: &'cx Context, index: u32) -> Result<Option<Value<'cx>>> {
		self.arr.get(cx, index)
	}

	/// Gets the value at the given index of the [Array] as a Rust type.
	/// Returns [None] if there is no value at the given index or conversion to the Rust type fails.
	pub fn get_as<'cx, T: FromValue<'cx>>(
		&self, cx: &'cx Context, index: u32, strict: bool, config: T::Config,
	) -> Result<Option<T>> {
		self.arr.get_as(cx, index, strict, config)
	}

	/// Gets the descriptor at the given index of the [Array].
	/// Returns [None] if there is no value at the given index.
	pub fn get_descriptor<'cx>(&self, cx: &'cx Context, index: u32) -> Result<Option<PropertyDescriptor<'cx>>> {
		self.arr.get_descriptor(cx, index)
	}

	/// Sets the [Value] at the given index of the [Array].
	/// Returns `false` if the element cannot be set.
	pub fn set(&self, cx: &Context, index: u32, value: &Value) -> bool {
		self.arr.set(cx, index, value)
	}

	/// Sets the Rust type at the given index of the [Array].
	/// Returns `false` if the element cannot be set.
	pub fn set_as<'cx, T: ToValue<'cx> + ?Sized>(&self, cx: &'cx Context, index: u32, value: &T) -> bool {
		self.arr.set_as(cx, index, value)
	}

	/// Defines the [Value] at the given index of the [Array] with the given attributes.
	/// Returns `false` if the element cannot be defined.
	pub fn define(&self, cx: &Context, index: u32, value: &Value, attrs: PropertyFlags) -> bool {
		self.arr.define(cx, index, value, attrs)
	}

	/// Defines the Rust type at the given index of the [Array] with the given attributes.
	/// Returns `false` if the element cannot be defined.
	pub fn define_as<'cx, T: ToValue<'cx> + ?Sized>(
		&self, cx: &'cx Context, index: u32, value: &T, attrs: PropertyFlags,
	) -> bool {
		self.arr.define_as(cx, index, value, attrs)
	}

	/// Deletes the [JSVal] at the given index.
	/// Returns `false` if the element cannot be deleted.
	pub fn delete(&self, cx: &Context, index: u32) -> bool {
		self.arr.delete(cx, index)
	}

	pub fn indices<'cx>(&self, cx: &'cx Context, flags: Option<IteratorFlags>) -> ArrayIndicesIter<'cx> {
		ArrayIndicesIter(self.arr.keys(cx, flags))
	}

	pub fn iter<'cx, 's>(&'s self, cx: &'cx Context, flags: Option<IteratorFlags>) -> ArrayIter<'cx, 's>
	where
		'a: 'cx,
	{
		ArrayIter::new(cx, self, self.indices(cx, flags))
	}

	/// Checks if a [*mut JSObject] is an array.
	pub fn is_array_raw(cx: &Context, object: *mut JSObject) -> bool {
		rooted!(in(cx.as_ptr()) let object = object);
		let mut is_array = false;
		unsafe { IsArray(cx.as_ptr(), object.handle().into(), &mut is_array) && is_array }
	}

	/// Checks if a [*mut JSObject] is an array.
	pub fn is_array(cx: &Context, object: &Local<*mut JSObject>) -> bool {
		let mut is_array = false;
		unsafe { IsArray(cx.as_ptr(), object.handle().into(), &mut is_array) && is_array }
	}

	pub fn as_object(&self) -> &Object<'a> {
		&self.arr
	}

	pub fn into_local(self) -> Local<'a, *mut JSObject> {
		self.arr.into_local()
	}
}

impl<'a> Deref for Array<'a> {
	type Target = Local<'a, *mut JSObject>;

	fn deref(&self) -> &Self::Target {
		&self.arr
	}
}

impl DerefMut for Array<'_> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.arr
	}
}

pub struct ArrayIndicesIter<'cx>(ObjectKeysIter<'cx>);

impl Iterator for ArrayIndicesIter<'_> {
	type Item = u32;

	fn next(&mut self) -> Option<u32> {
		for key in self.0.by_ref() {
			let key = key.get();
			if key.is_int() {
				return Some(key.to_int() as u32);
			}
		}
		None
	}

	fn size_hint(&self) -> (usize, Option<usize>) {
		(0, self.0.size_hint().1)
	}
}

impl DoubleEndedIterator for ArrayIndicesIter<'_> {
	fn next_back(&mut self) -> Option<u32> {
		while let Some(key) = self.0.next_back() {
			let key = key.get();
			if key.is_int() {
				return Some(key.to_int() as u32);
			}
		}
		None
	}
}

impl FusedIterator for ArrayIndicesIter<'_> {}

pub struct ArrayIter<'cx, 'a> {
	cx: &'cx Context,
	array: &'a Array<'cx>,
	indices: ArrayIndicesIter<'cx>,
}

impl<'cx, 'a> ArrayIter<'cx, 'a> {
	fn new(cx: &'cx Context, array: &'a Array<'cx>, indices: ArrayIndicesIter<'cx>) -> ArrayIter<'cx, 'a> {
		ArrayIter { cx, array, indices }
	}
}

impl<'cx> Iterator for ArrayIter<'cx, '_> {
	type Item = (u32, Result<Value<'cx>>);

	fn next(&mut self) -> Option<Self::Item> {
		self.indices.next().map(|index| {
			let value = self.array.get(self.cx, index).transpose().unwrap();
			(index, value)
		})
	}

	fn size_hint(&self) -> (usize, Option<usize>) {
		self.indices.size_hint()
	}
}

impl DoubleEndedIterator for ArrayIter<'_, '_> {
	fn next_back(&mut self) -> Option<Self::Item> {
		self.indices.next_back().map(|index| {
			let value = self.array.get(self.cx, index).transpose().unwrap();
			(index, value)
		})
	}
}

impl FusedIterator for ArrayIter<'_, '_> {}

#[cfg(test)]
mod tests {
	use crate::flags::PropertyFlags;
	use crate::utils::test::TestRuntime;
	use crate::{Array, Result, Value};

	#[test]
	fn new() {
		let rt = TestRuntime::new();
		let cx = &rt.cx;

		let array = Array::new(cx);
		assert!(!array.handle().get().is_null());
		assert!(array.is_empty(cx));

		let array = Array::new_with_length(cx, 8);
		assert!(!array.handle().get().is_null());
		assert_eq!(array.len(cx), 8);
	}

	#[test]
	fn get_set() {
		let rt = TestRuntime::new();
		let cx = &rt.cx;

		const VALUE: i32 = 6;
		let array = Array::new(cx);
		assert!(array.set(cx, 0, &Value::i32(cx, VALUE)));
		assert_eq!(array.len(cx), 1);

		assert!(array.has(cx, 0));
		let result = array.get(cx, 0).unwrap().unwrap();
		assert!(result.handle().is_int32());
		assert_eq!(result.handle().to_int32(), VALUE);

		assert!(!array.has(cx, 1));
		let result = array.get(cx, 1).unwrap();
		assert!(result.is_none());
	}

	#[test]
	fn get_set_as() {
		let rt = TestRuntime::new();
		let cx = &rt.cx;

		let array = Array::new(cx);

		const STRING_VALUE: &'static str = "spiderfire";
		assert!(array.set_as(cx, 0, &STRING_VALUE));
		assert_eq!(array.len(cx), 1);

		assert!(array.has(cx, 0));
		let result: String = array.get_as(cx, 0, true, ()).unwrap().unwrap();
		assert_eq!(result, STRING_VALUE);

		const BOOL_VALUE: bool = true;
		assert!(array.set_as(cx, 1, &BOOL_VALUE));
		assert_eq!(array.len(cx), 2);

		assert!(array.has(cx, 1));
		let result: bool = array.get_as(cx, 1, true, ()).unwrap().unwrap();
		assert_eq!(result, BOOL_VALUE);
	}

	#[test]
	fn define_descriptor() {
		let rt = TestRuntime::new();
		let cx = &rt.cx;

		let array = Array::new(cx);

		const VALUE: i32 = 6;
		let flags = PropertyFlags::READ_ONLY | PropertyFlags::PERMANENT;
		assert!(array.define(cx, 0, &Value::i32(cx, VALUE), flags));

		let result = array.get(&cx, 0).unwrap().unwrap();
		assert!(result.handle().is_int32());
		assert_eq!(result.handle().to_int32(), VALUE);

		let desc = array.get_descriptor(&cx, 0).unwrap().unwrap();
		assert!(!desc.is_writable());
		assert!(!desc.is_configurable());

		assert!(array.set(cx, 0, &Value::i32(cx, VALUE + 1)));
		let result = array.get(&cx, 0).unwrap().unwrap();
		assert!(result.handle().is_int32());
		assert_eq!(result.handle().to_int32(), VALUE);
	}

	#[test]
	fn delete_sparse() {
		let rt = TestRuntime::new();
		let cx = &rt.cx;

		let array = Array::new(cx);
		array.set_as(cx, 0, &0);
		array.set_as(cx, 1, &1);
		assert_eq!(array.len(cx), 2);

		assert!(array.delete(cx, 0));

		assert!(!array.has(cx, 0));
		assert!(array.has(cx, 1));
		assert_eq!(array.len(cx), 2);
	}

	#[test]
	fn iter() {
		let rt = TestRuntime::new();
		let cx = &rt.cx;

		let array = Array::new(cx);
		assert!(array.set_as(cx, 0, &0));
		assert!(array.set_as(cx, 1, &1));
		assert!(array.set_as(cx, 3, &3));

		let indices: Vec<_> = array.indices(cx, None).collect();
		assert_eq!(indices, [0, 1, 3]);

		let pairs: Vec<_> = array
			.iter(cx, None)
			.map(|(k, v)| Ok((k, v?.handle().to_int32())))
			.collect::<Result<_>>()
			.unwrap();
		assert_eq!(pairs, [(0, 0), (1, 1), (3, 3)]);
	}
}
