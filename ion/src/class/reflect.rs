/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::any::TypeId;
use std::ptr;

use mozjs::gc::Traceable;
use mozjs::jsapi::{Heap, JSClass, JSObject, JSTracer};
use mozjs::rust::{get_object_class, Handle};

use crate::class::NativeClass;

pub trait NativeObject: Traceable + Sized + 'static {
	fn reflector(&self) -> &Reflector;
}

pub trait NativeMutObject: NativeObject {
	fn init_reflector(&self, obj: *mut JSObject) {
		self.reflector().set(obj);
	}
}

pub unsafe trait DerivedFrom<T: Castable>: Castable {}

unsafe impl<T: Castable> DerivedFrom<T> for T {}

pub trait Castable: NativeObject {
	fn is<T>(&self) -> bool
	where
		T: NativeObject,
	{
		let class = unsafe { get_object_class(self.reflector().get()) };
		let native_class = class.cast::<NativeClass>();
		let mut proto_chain = unsafe { (*native_class).prototype_chain.iter() };
		let mut is = false;
		while let Some(Some(proto)) = proto_chain.next() {
			is |= proto.type_id() == TypeId::of::<T>()
		}
		is
	}

	fn upcast<T: Castable>(&self) -> &T
	where
		Self: DerivedFrom<T>,
	{
		unsafe { &*(self as *const Self).cast::<T>() }
	}

	fn downcast<T>(&self) -> Option<&T>
	where
		T: DerivedFrom<Self> + NativeObject,
	{
		self.is::<T>().then(|| unsafe { &*(self as *const Self).cast::<T>() })
	}
}

#[derive(Default)]
pub struct Reflector(Heap<*mut JSObject>);

impl Reflector {
	pub fn new() -> Reflector {
		Reflector::default()
	}

	pub fn get(&self) -> *mut JSObject {
		self.0.get()
	}

	pub fn handle(&self) -> Handle<*mut JSObject> {
		unsafe { Handle::from_raw(self.0.handle()) }
	}

	pub fn set(&self, obj: *mut JSObject) {
		assert!(self.0.get().is_null());
		assert!(!obj.is_null());
		self.0.set(obj);
	}

	#[doc(hidden)]
	pub fn __ion_native_class() -> &'static NativeClass {
		&NativeClass {
			base: JSClass {
				name: ptr::null(),
				flags: 0,
				cOps: ptr::null(),
				spec: ptr::null(),
				ext: ptr::null(),
				oOps: ptr::null(),
			},
			prototype_chain: [None; 8],
		}
	}
}

unsafe impl Traceable for Reflector {
	unsafe fn trace(&self, trc: *mut JSTracer) {
		unsafe {
			self.0.trace(trc);
		}
	}
}

impl NativeObject for Reflector {
	fn reflector(&self) -> &Reflector {
		self
	}
}

impl NativeMutObject for Reflector {}

impl Castable for Reflector {}
