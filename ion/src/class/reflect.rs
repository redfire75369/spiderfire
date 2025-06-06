/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::any::{Any as _, TypeId};
use std::ptr;

use mozjs::gc::Traceable;
use mozjs::jsapi::{Heap, JSObject, JSTracer};
use mozjs::rust::{Handle, get_object_class};

use crate::class::{NativeClass, PrototypeChain};

pub trait NativeObject: Traceable + Sized + 'static {
	fn reflector(&self) -> &Reflector;
}

pub unsafe trait DerivedFrom<T: Castable>: Castable {}

unsafe impl<T: Castable> DerivedFrom<T> for T {}

pub trait Castable: NativeObject {
	fn is<T>(&self) -> bool
	where
		T: NativeObject,
	{
		let class = unsafe { get_object_class(self.reflector().get()) };
		if class.is_null() {
			return false;
		}

		unsafe {
			(*class.cast::<NativeClass>())
				.prototype_chain
				.iter()
				.any(|proto| proto.type_id() == TypeId::of::<T>())
		}
	}

	fn upcast<T: Castable>(&self) -> &T
	where
		Self: DerivedFrom<T>,
	{
		unsafe { &*ptr::from_ref(self).cast::<T>() }
	}

	fn downcast<T>(&self) -> Option<&T>
	where
		T: DerivedFrom<Self> + NativeObject,
	{
		self.is::<T>().then(|| unsafe { &*ptr::from_ref(self).cast::<T>() })
	}
}

#[derive(Debug, Default)]
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

	pub(super) fn set(&self, obj: *mut JSObject) {
		assert!(self.0.get().is_null());
		assert!(!obj.is_null());
		self.0.set(obj);
	}

	#[doc(hidden)]
	pub const fn __ion_native_prototype_chain() -> PrototypeChain {
		PrototypeChain::new()
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

impl Castable for Reflector {}
