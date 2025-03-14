/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::collections::HashMap;
use std::iter::FusedIterator;
use std::mem::MaybeUninit;
use std::ops::{Deref, DerefMut};
use std::slice;

use mozjs::jsapi::{
	CurrentGlobalOrNull, ESClass, GetBuiltinClass, GetPropertyKeys, JS_DefineFunctionById, JS_DefineFunctions,
	JS_DefineFunctionsWithHelp, JS_DefineProperties, JS_DefinePropertyById2, JS_DeletePropertyById, JS_GetPropertyById,
	JS_GetPropertyDescriptorById, JS_HasOwnPropertyById, JS_HasPropertyById, JS_NewPlainObject, JS_SetPropertyById,
	JSFunctionSpec, JSFunctionSpecWithHelp, JSObject, JSPropertySpec, PropertyKey as JSPropertyKey, Unbox,
};
use mozjs::jsval::NullValue;
use mozjs::rust::IdVector;

use crate::conversions::{FromValue, ToPropertyKey, ToValue};
use crate::flags::{IteratorFlags, PropertyFlags};
use crate::function::NativeFunction;
use crate::{Context, Error, Exception, Function, Local, OwnedKey, PropertyDescriptor, PropertyKey, Result, Value};

/// Represents an [Object] in the JS Runtime.
///
/// Refer to [MDN](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Object) for more details.
#[derive(Debug)]
pub struct Object<'o> {
	obj: Local<'o, *mut JSObject>,
}

impl<'o> Object<'o> {
	/// Creates a plain empty [Object].
	pub fn new(cx: &'o Context) -> Object<'o> {
		Object::from(cx.root(unsafe { JS_NewPlainObject(cx.as_ptr()) }))
	}

	/// Creates a `null` "Object".
	///
	/// Most operations on this will result in an error, so be wary of where it is used.
	pub fn null(cx: &'o Context) -> Object<'o> {
		Object::from(cx.root(NullValue().to_object_or_null()))
	}

	/// Returns the current global object or `null` if one has not been initialised yet.
	pub fn global(cx: &'o Context) -> Object<'o> {
		Object::from(cx.root(unsafe { CurrentGlobalOrNull(cx.as_ptr()) }))
	}

	/// Checks if the [Object] has a value at the given key.
	pub fn has<'cx, K: ToPropertyKey<'cx>>(&self, cx: &'cx Context, key: K) -> bool {
		let key = key.to_key(cx).unwrap();
		let mut found = false;
		if unsafe { JS_HasPropertyById(cx.as_ptr(), self.handle().into(), key.handle().into(), &mut found) } {
			found
		} else {
			Exception::clear(cx);
			false
		}
	}

	/// Checks if the [Object] has its own value at the given key.
	///
	/// An object owns its properties if they are not inherited from a prototype.
	pub fn has_own<'cx, K: ToPropertyKey<'cx>>(&self, cx: &'cx Context, key: K) -> bool {
		let key = key.to_key(cx).unwrap();
		let mut found = false;
		if unsafe { JS_HasOwnPropertyById(cx.as_ptr(), self.handle().into(), key.handle().into(), &mut found) } {
			found
		} else {
			Exception::clear(cx);
			false
		}
	}

	/// Gets the [Value] at the given key of the [Object].
	///
	/// Returns [None] if there is no value at the given key.
	pub fn get<'cx, K: ToPropertyKey<'cx>>(&self, cx: &'cx Context, key: K) -> Result<Option<Value<'cx>>> {
		let key = key.to_key(cx).unwrap();
		if self.has(cx, &key) {
			let mut rval = Value::undefined(cx);
			let res = unsafe {
				JS_GetPropertyById(
					cx.as_ptr(),
					self.handle().into(),
					key.handle().into(),
					rval.handle_mut().into(),
				)
			};

			if res { Ok(Some(rval)) } else { Err(Error::none()) }
		} else {
			Ok(None)
		}
	}

	/// Gets the value at the given key of the [Object]. as a Rust type.
	/// Returns [None] if the object does not contain the key or conversion to the Rust type fails.
	pub fn get_as<'cx, K: ToPropertyKey<'cx>, T: FromValue<'cx>>(
		&self, cx: &'cx Context, key: K, strict: bool, config: T::Config,
	) -> Result<Option<T>> {
		self.get(cx, key)?.map(|val| T::from_value(cx, &val, strict, config)).transpose()
	}

	/// Gets the descriptor at the given key of the [Object].
	/// Returns [None] if the object does not contain the key.
	pub fn get_descriptor<'cx, K: ToPropertyKey<'cx>>(
		&self, cx: &'cx Context, key: K,
	) -> Result<Option<PropertyDescriptor<'cx>>> {
		let key = key.to_key(cx).unwrap();
		if self.has(cx, &key) {
			let mut desc = PropertyDescriptor::empty(cx);
			let mut holder = Object::null(cx);
			let mut is_none = true;
			let res = unsafe {
				JS_GetPropertyDescriptorById(
					cx.as_ptr(),
					self.handle().into(),
					key.handle().into(),
					desc.handle_mut().into(),
					holder.handle_mut().into(),
					&mut is_none,
				)
			};

			if !res {
				Err(Error::none())
			} else if is_none {
				Ok(None)
			} else {
				Ok(Some(desc))
			}
		} else {
			Ok(None)
		}
	}

	/// Sets the [Value] at the given key of the [Object].
	///
	/// Returns `false` if the property cannot be set.
	pub fn set<'cx, K: ToPropertyKey<'cx>>(&self, cx: &'cx Context, key: K, value: &Value) -> bool {
		let key = key.to_key(cx).unwrap();
		unsafe {
			JS_SetPropertyById(
				cx.as_ptr(),
				self.handle().into(),
				key.handle().into(),
				value.handle().into(),
			)
		}
	}

	/// Sets the Rust type at the given key of the [Object].
	///
	/// Returns `false` if the property cannot be set.
	pub fn set_as<'cx, K: ToPropertyKey<'cx>, T: ToValue<'cx> + ?Sized>(
		&self, cx: &'cx Context, key: K, value: &T,
	) -> bool {
		self.set(cx, key, &value.as_value(cx))
	}

	/// Defines the [Value] at the given key of the [Object] with the given attributes.
	///
	/// Returns `false` if the property cannot be defined.
	pub fn define<'cx, K: ToPropertyKey<'cx>>(
		&self, cx: &'cx Context, key: K, value: &Value, attrs: PropertyFlags,
	) -> bool {
		let key = key.to_key(cx).unwrap();
		unsafe {
			JS_DefinePropertyById2(
				cx.as_ptr(),
				self.handle().into(),
				key.handle().into(),
				value.handle().into(),
				u32::from(attrs.bits()),
			)
		}
	}

	/// Defines the Rust type at the given key of the [Object] with the given attributes.
	///
	/// Returns `false` if the property cannot be defined.
	pub fn define_as<'cx, K: ToPropertyKey<'cx>, T: ToValue<'cx> + ?Sized>(
		&self, cx: &'cx Context, key: K, value: &T, attrs: PropertyFlags,
	) -> bool {
		self.define(cx, key, &value.as_value(cx), attrs)
	}

	/// Defines a method with the given name, and the given number of arguments and attributes on the [Object].
	///
	/// Parameters are similar to [create_function_spec](crate::spec::create_function_spec).
	pub fn define_method<'cx, K: ToPropertyKey<'cx>>(
		&self, cx: &'cx Context, key: K, method: NativeFunction, nargs: u32, attrs: PropertyFlags,
	) -> Function<'cx> {
		let key = key.to_key(cx).unwrap();
		cx.root(unsafe {
			JS_DefineFunctionById(
				cx.as_ptr(),
				self.handle().into(),
				key.handle().into(),
				Some(method),
				nargs,
				u32::from(attrs.bits()),
			)
		})
		.into()
	}

	/// Defines methods on the objects using the given [specs](JSFunctionSpec).
	///
	/// The final element of the `methods` slice must be `JSFunctionSpec::ZERO`.
	#[cfg_attr(
		feature = "macros",
		doc = "\nThey can be created through [function_spec](crate::function_spec)."
	)]
	pub unsafe fn define_methods(&self, cx: &Context, methods: &[JSFunctionSpec]) -> bool {
		unsafe { JS_DefineFunctions(cx.as_ptr(), self.handle().into(), methods.as_ptr()) }
	}

	/// Defines methods on the objects using the given [specs](JSFunctionSpecWithHelp), with help.
	///
	/// The final element of the `methods` slice must be `JSFunctionSpecWithHelp::ZERO`.
	pub unsafe fn define_methods_with_help(&self, cx: &Context, methods: &[JSFunctionSpecWithHelp]) -> bool {
		unsafe { JS_DefineFunctionsWithHelp(cx.as_ptr(), self.handle().into(), methods.as_ptr()) }
	}

	/// Defines properties on the object using the given [specs](JSPropertySpec).
	///
	/// The final element of the `properties` slice must be `JSPropertySpec::ZERO`.
	pub unsafe fn define_properties(&self, cx: &Context, properties: &[JSPropertySpec]) -> bool {
		unsafe { JS_DefineProperties(cx.as_ptr(), self.handle().into(), properties.as_ptr()) }
	}

	/// Deletes the [Value] at the given index.
	///
	/// Returns `false` if the element cannot be deleted.
	pub fn delete<'cx, K: ToPropertyKey<'cx>>(&self, cx: &'cx Context, key: K) -> bool {
		let key = key.to_key(cx).unwrap();
		let mut result = MaybeUninit::uninit();
		unsafe {
			JS_DeletePropertyById(
				cx.as_ptr(),
				self.handle().into(),
				key.handle().into(),
				result.as_mut_ptr(),
			)
		}
	}

	/// Gets the builtin class of the object as described in the ECMAScript specification.
	///
	/// Returns [ESClass::Other] for other projects or proxies that cannot be unwrapped.
	pub fn get_builtin_class(&self, cx: &Context) -> ESClass {
		let mut class = ESClass::Other;
		unsafe {
			GetBuiltinClass(cx.as_ptr(), self.handle().into(), &mut class);
		}
		class
	}

	/// Returns the builtin class of the object if it a wrapper around a primitive.
	///
	/// The boxed types are `Boolean`, `Number`, `String` and `BigInt`
	pub fn is_boxed_primitive(&self, cx: &Context) -> Option<ESClass> {
		let class = self.get_builtin_class(cx);
		match class {
			ESClass::Boolean | ESClass::Number | ESClass::String | ESClass::BigInt => Some(class),
			_ => None,
		}
	}

	/// Unboxes primitive wrappers. See [Object::is_boxed_primitive] for details.
	pub fn unbox_primitive<'cx>(&self, cx: &'cx Context) -> Option<Value<'cx>> {
		if self.is_boxed_primitive(cx).is_some() {
			let mut rval = Value::undefined(cx);
			if unsafe { Unbox(cx.as_ptr(), self.handle().into(), rval.handle_mut().into()) } {
				return Some(rval);
			}
		}
		None
	}

	/// Returns an iterator of the keys of the [Object].
	/// Each key can be a [String], [Symbol](crate::symbol) or integer.
	pub fn keys<'cx>(&self, cx: &'cx Context, flags: Option<IteratorFlags>) -> ObjectKeysIter<'cx> {
		let flags = flags.unwrap_or(IteratorFlags::OWN_ONLY);
		let mut ids = unsafe { IdVector::new(cx.as_ptr()) };
		unsafe { GetPropertyKeys(cx.as_ptr(), self.handle().into(), flags.bits(), ids.handle_mut()) };
		ObjectKeysIter::new(cx, ids)
	}

	pub fn iter<'cx, 's>(&'s self, cx: &'cx Context, flags: Option<IteratorFlags>) -> ObjectIter<'cx, 's>
	where
		'o: 'cx,
	{
		ObjectIter::new(self, self.keys(cx, flags))
	}

	pub fn to_hashmap<'cx>(
		&self, cx: &'cx Context, flags: Option<IteratorFlags>,
	) -> Result<HashMap<OwnedKey<'cx>, Value<'cx>>>
	where
		'o: 'cx,
	{
		self.iter(cx, flags).map(|(k, v)| Ok((k.to_owned_key(cx)?, v?))).collect()
	}

	pub fn into_local(self) -> Local<'o, *mut JSObject> {
		self.obj
	}
}

impl<'o> From<Local<'o, *mut JSObject>> for Object<'o> {
	fn from(obj: Local<'o, *mut JSObject>) -> Object<'o> {
		Object { obj }
	}
}

impl<'o> Deref for Object<'o> {
	type Target = Local<'o, *mut JSObject>;

	fn deref(&self) -> &Self::Target {
		&self.obj
	}
}

impl DerefMut for Object<'_> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.obj
	}
}

pub struct ObjectKeysIter<'cx> {
	cx: &'cx Context,
	slice: &'static [JSPropertyKey],
	keys: IdVector,
	index: usize,
	count: usize,
}

impl<'cx> ObjectKeysIter<'cx> {
	fn new(cx: &'cx Context, keys: IdVector) -> ObjectKeysIter<'cx> {
		let keys_slice = &*keys;
		let count = keys_slice.len();
		let keys_slice = unsafe { slice::from_raw_parts(keys_slice.as_ptr(), count) };
		ObjectKeysIter {
			cx,
			slice: keys_slice,
			keys,
			index: 0,
			count,
		}
	}

	pub fn into_owned(self) -> ObjectOwnedKeysIter<'cx> {
		ObjectOwnedKeysIter { iter: self }
	}
}

impl<'cx> Iterator for ObjectKeysIter<'cx> {
	type Item = PropertyKey<'cx>;

	fn next(&mut self) -> Option<Self::Item> {
		if self.index < self.count {
			let key = &self.slice[self.index];
			self.index += 1;
			Some(self.cx.root(*key).into())
		} else {
			None
		}
	}

	fn size_hint(&self) -> (usize, Option<usize>) {
		(self.count - self.index, Some(self.count - self.index))
	}
}

impl DoubleEndedIterator for ObjectKeysIter<'_> {
	fn next_back(&mut self) -> Option<Self::Item> {
		if self.index < self.count {
			self.count -= 1;
			let key = &self.keys[self.count];
			Some(self.cx.root(*key).into())
		} else {
			None
		}
	}
}

impl ExactSizeIterator for ObjectKeysIter<'_> {
	fn len(&self) -> usize {
		self.count - self.index
	}
}

impl FusedIterator for ObjectKeysIter<'_> {}

pub struct ObjectOwnedKeysIter<'cx> {
	iter: ObjectKeysIter<'cx>,
}

impl<'cx> Iterator for ObjectOwnedKeysIter<'cx> {
	type Item = Result<OwnedKey<'cx>>;

	fn next(&mut self) -> Option<Result<OwnedKey<'cx>>> {
		self.iter.next().map(|key| key.to_owned_key(self.iter.cx))
	}

	fn size_hint(&self) -> (usize, Option<usize>) {
		self.iter.size_hint()
	}
}

impl DoubleEndedIterator for ObjectOwnedKeysIter<'_> {
	fn next_back(&mut self) -> Option<Self::Item> {
		self.iter.next_back().map(|key| key.to_owned_key(self.iter.cx))
	}
}

impl ExactSizeIterator for ObjectOwnedKeysIter<'_> {
	fn len(&self) -> usize {
		self.iter.len()
	}
}

impl FusedIterator for ObjectOwnedKeysIter<'_> {}

pub struct ObjectIter<'cx, 'o> {
	object: &'o Object<'cx>,
	keys: ObjectKeysIter<'cx>,
}

impl<'cx, 'o> ObjectIter<'cx, 'o> {
	fn new(object: &'o Object<'cx>, keys: ObjectKeysIter<'cx>) -> ObjectIter<'cx, 'o> {
		ObjectIter { object, keys }
	}
}

impl<'cx> Iterator for ObjectIter<'cx, '_> {
	type Item = (PropertyKey<'cx>, Result<Value<'cx>>);

	fn next(&mut self) -> Option<Self::Item> {
		self.keys.next().map(|key| {
			let value = self.object.get(self.keys.cx, &key).transpose().unwrap();
			(key, value)
		})
	}

	fn size_hint(&self) -> (usize, Option<usize>) {
		self.keys.size_hint()
	}
}

impl DoubleEndedIterator for ObjectIter<'_, '_> {
	fn next_back(&mut self) -> Option<Self::Item> {
		self.keys.next_back().map(|key| {
			let value = self.object.get(self.keys.cx, &key).transpose().unwrap();
			(key, value)
		})
	}
}

impl ExactSizeIterator for ObjectIter<'_, '_> {
	fn len(&self) -> usize {
		self.keys.len()
	}
}

impl FusedIterator for ObjectIter<'_, '_> {}

#[cfg(test)]
mod tests {
	use crate::conversions::FromValue;
	use crate::flags::{IteratorFlags, PropertyFlags};
	use crate::symbol::WellKnownSymbolCode;
	use crate::utils::test::TestRuntime;
	use crate::{Context, Object, OwnedKey, Symbol, Value};

	type Property = (&'static str, i32);

	const SET: Property = ("set_key", 0);
	const DEFINE: Property = ("def_key", 1);
	const ENUMERABLE: Property = ("enum_key", 2);

	const ENUMERABLE_PROPERTIES: [Property; 2] = [SET, ENUMERABLE];
	const PROPERTIES: [Property; 3] = [SET, DEFINE, ENUMERABLE];

	fn create_object(cx: &Context) -> Object {
		let object = Object::new(cx);

		assert!(object.set(cx, SET.0, &Value::i32(cx, SET.1)));
		assert!(object.define(cx, DEFINE.0, &Value::i32(cx, DEFINE.1), PropertyFlags::CONSTANT));
		assert!(object.define(cx, ENUMERABLE.0, &Value::i32(cx, ENUMERABLE.1), PropertyFlags::all()));

		object
	}

	fn create_mixed_object(cx: &Context) -> Object {
		let object = Object::new(cx);

		assert!(object.set(cx, 16, &Value::i32(cx, 16)));
		assert!(object.set(cx, SET.0, &Value::i32(cx, SET.1)));
		assert!(object.set(cx, WellKnownSymbolCode::ToStringTag, &Value::string(cx, "Mixed")));

		object
	}

	#[test]
	fn global() {
		let rt = TestRuntime::new();
		let cx = &rt.cx;

		let global = Object::global(cx);
		assert_eq!(rt.global, global.handle().get());
	}

	#[test]
	fn property() {
		let rt = TestRuntime::new();
		let cx = &rt.cx;

		let object = create_object(cx);

		assert!(object.has(cx, SET.0));
		assert!(object.has_own(cx, DEFINE.0));

		let value = object.get(cx, SET.0).unwrap().unwrap();
		assert_eq!(SET.1, value.handle().to_int32());

		let descriptor = object.get_descriptor(cx, SET.0).unwrap().unwrap();
		assert!(descriptor.is_configurable());
		assert!(descriptor.is_enumerable());
		assert!(descriptor.is_writable());
		assert!(!descriptor.is_resolving());
		assert_eq!(SET.1, descriptor.value(cx).unwrap().handle().to_int32());

		let value = object.get(cx, DEFINE.0).unwrap().unwrap();
		assert_eq!(DEFINE.1, value.handle().to_int32());

		let descriptor = object.get_descriptor(cx, DEFINE.0).unwrap().unwrap();
		assert!(!descriptor.is_configurable());
		assert!(!descriptor.is_enumerable());
		assert!(!descriptor.is_writable());
		assert!(!descriptor.is_resolving());
		assert_eq!(DEFINE.1, descriptor.value(cx).unwrap().handle().to_int32());

		assert!(object.delete(cx, SET.0));
		assert!(object.delete(cx, DEFINE.0));
		assert!(object.get(cx, SET.0).unwrap().is_none());
		assert!(object.get(cx, DEFINE.0).unwrap().is_some());
	}

	#[test]
	fn iter() {
		let rt = TestRuntime::new();
		let cx = &rt.cx;

		let object = create_object(cx);

		let properties = [
			(ENUMERABLE_PROPERTIES.as_slice(), None),
			(
				PROPERTIES.as_slice(),
				Some(IteratorFlags::OWN_ONLY | IteratorFlags::HIDDEN),
			),
		];

		for (properties, flags) in properties {
			for (i, key) in object.keys(cx, flags).into_owned().enumerate() {
				assert_eq!(OwnedKey::String(String::from(properties[i].0)), key.unwrap());
			}

			for (i, (key, value)) in object.iter(cx, flags).enumerate() {
				assert_eq!(
					OwnedKey::String(String::from(properties[i].0)),
					key.to_owned_key(cx).unwrap()
				);
				assert_eq!(properties[i].1, value.unwrap().handle().to_int32());
			}
		}
	}

	#[test]
	fn mixed_iter() {
		let rt = TestRuntime::new();
		let cx = &rt.cx;

		let object = create_mixed_object(cx);
		let flags = Some(IteratorFlags::SYMBOLS);

		let mut keys = object.keys(cx, flags).into_owned();
		assert_eq!(OwnedKey::Int(16), keys.next().unwrap().unwrap());
		assert_eq!(OwnedKey::String(String::from(SET.0)), keys.next().unwrap().unwrap());
		assert_eq!(
			OwnedKey::Symbol(Symbol::well_known(cx, WellKnownSymbolCode::ToStringTag)),
			keys.next().unwrap().unwrap()
		);

		let mut values = object.iter(cx, flags);
		assert_eq!(16, values.next().unwrap().1.unwrap().handle().to_int32());
		assert_eq!(SET.1, values.next().unwrap().1.unwrap().handle().to_int32());
		assert_eq!(
			"Mixed",
			crate::String::from_value(cx, &values.next().unwrap().1.unwrap(), true, ())
				.unwrap()
				.to_owned(cx)
				.unwrap()
		);
	}
}
