/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::any::TypeId;
use std::collections::hash_map::Entry;
use std::ffi::CStr;
use std::ptr;

use ion_proc::Traceable;
use mozjs::error::throw_type_error;
use mozjs::gc::{HandleObject, Traceable};
use mozjs::glue::JS_GetReservedSlot;
use mozjs::jsapi::{
	GCContext, Heap, JS_GetConstructor, JS_HasInstance, JS_InitClass, JS_InstanceOf, JS_NewObjectWithGivenProto,
	JS_SetReservedSlot, JSContext, JSFunction, JSFunctionSpec, JSObject, JSPropertySpec, JSTracer,
};
use mozjs::jsval::{JSVal, NullValue, PrivateValue, UndefinedValue};
use mozjs::rust::get_object_class;

pub use crate::class::native::{MAX_PROTO_CHAIN_LENGTH, NativeClass, PrototypeChain, TypeIdWrapper};
pub use crate::class::reflect::{Castable, DerivedFrom, NativeObject, Reflector};
use crate::conversions::{IntoValue, ToValue as _};
use crate::function::{__handle_native_constructor_result, NativeFunction};
use crate::{
	Arguments, Context, Error, ErrorKind, Exception, Function, Local, Object, Result, ResultExc, Value,
	class_num_reserved_slots,
};

mod native;
mod reflect;

/// Stores information about a native class created for JS.
#[derive(Debug, Traceable)]
#[trace(crate = crate)]
pub struct ClassInfo {
	#[trace(no_trace)]
	class: &'static NativeClass,
	pub constructor: Box<Heap<*mut JSFunction>>,
	pub prototype: Box<Heap<*mut JSObject>>,
}

pub trait ClassDefinition: NativeObject {
	fn class() -> &'static NativeClass;

	fn proto_class() -> Option<&'static NativeClass> {
		None
	}

	fn parent_prototype(_: &Context) -> Option<Local<'_, *mut JSObject>> {
		None
	}

	fn constructor() -> (Option<NativeFunction>, u32);

	fn functions() -> Option<&'static [JSFunctionSpec]> {
		None
	}

	fn properties() -> Option<&'static [JSPropertySpec]> {
		None
	}

	fn static_functions() -> Option<&'static [JSFunctionSpec]> {
		None
	}

	fn static_properties() -> Option<&'static [JSPropertySpec]> {
		None
	}

	fn init_class<'cx>(cx: &'cx Context, object: &Object) -> (bool, &'cx ClassInfo) {
		let infos = unsafe { &mut (*cx.get_inner_data().as_ptr()).class_infos };

		match infos.entry(TypeId::of::<Self>()) {
			Entry::Occupied(o) => (false, o.into_mut()),
			Entry::Vacant(entry) => {
				let proto_class = Self::proto_class().map_or_else(ptr::null, |class| &raw const class.base);
				let parent_proto = Self::parent_prototype(cx).map_or_else(HandleObject::null, |proto| proto.handle());

				let (constructor, nargs) = Self::constructor();

				let properties = Self::properties();
				let functions = Self::functions();
				let static_properties = Self::static_properties();
				let static_functions = Self::static_functions();

				assert!(has_zero_spec(properties));
				assert!(has_zero_spec(functions));
				assert!(has_zero_spec(static_properties));
				assert!(has_zero_spec(static_functions));

				let class = unsafe {
					JS_InitClass(
						cx.as_ptr(),
						object.handle().into(),
						proto_class,
						parent_proto.into(),
						Self::class().base.name,
						Some(constructor.unwrap_or(illegal_constructor)),
						nargs,
						unwrap_specs(properties),
						unwrap_specs(functions),
						unwrap_specs(static_properties),
						unwrap_specs(static_functions),
					)
				};
				let prototype = cx.root(class);

				let constructor = unsafe { JS_GetConstructor(cx.as_ptr(), prototype.handle().into()) };
				let constructor = Object::from(cx.root(constructor));
				let constructor = Function::from_object(cx, &constructor).unwrap();

				let class_info = ClassInfo {
					class: Self::class(),
					constructor: Heap::boxed(constructor.get()),
					prototype: Heap::boxed(prototype.get()),
				};

				(true, entry.insert(class_info))
			}
		}
	}

	fn new_raw_object(cx: &Context) -> *mut JSObject {
		let infos = unsafe { &mut (*cx.get_inner_data().as_ptr()).class_infos };
		let info = infos.get(&TypeId::of::<Self>()).expect("Uninitialised Class");
		unsafe {
			JS_NewObjectWithGivenProto(
				cx.as_ptr(),
				&raw const Self::class().base,
				Local::from_heap(&info.prototype).handle().into(),
			)
		}
	}

	fn new_object(cx: &Context, native: Box<Self>) -> *mut JSObject {
		let object = Self::new_raw_object(cx);
		unsafe {
			Self::set_private(object, native);
		}
		object
	}

	unsafe fn get_private_unchecked<'a>(object: &Object<'a>) -> &'a Self {
		unsafe {
			let mut value = UndefinedValue();
			JS_GetReservedSlot(object.handle().get(), 0, &raw mut value);
			&*(value.to_private().cast::<Self>())
		}
	}

	fn get_private<'a>(cx: &Context, object: &Object<'a>) -> Result<&'a Self> {
		check_private::<Self>(cx, object)?;
		Ok(unsafe { Self::get_private_unchecked(object) })
	}

	unsafe fn get_mut_private_unchecked<'a>(object: &Object<'a>) -> &'a mut Self {
		unsafe {
			let mut value = UndefinedValue();
			JS_GetReservedSlot(object.handle().get(), 0, &raw mut value);
			&mut *(value.to_private().cast_mut().cast::<Self>())
		}
	}

	fn get_mut_private<'a>(cx: &Context, object: &Object<'a>) -> Result<&'a mut Self> {
		check_private::<Self>(cx, object)?;
		Ok(unsafe { Self::get_mut_private_unchecked(object) })
	}

	unsafe fn set_private(object: *mut JSObject, native: Box<Self>) {
		native.reflector().set(object);
		unsafe {
			JS_SetReservedSlot(object, 0, &PrivateValue(Box::into_raw(native).cast_const().cast()));
		}
	}

	fn instance_of(cx: &Context, object: &Object) -> bool {
		unsafe {
			JS_InstanceOf(
				cx.as_ptr(),
				object.handle().into(),
				&raw const Self::class().base,
				ptr::null_mut(),
			)
		}
	}

	fn has_instance(cx: &Context, object: &Object) -> Result<bool> {
		let infos = unsafe { &mut (*cx.get_inner_data().as_ptr()).class_infos };
		let constructor =
			Function::from(cx.root(infos.get(&TypeId::of::<Self>()).expect("Uninitialised Class").constructor.get()))
				.to_object(cx);
		let object = object.as_value(cx);
		let mut has_instance = false;
		let result = unsafe {
			JS_HasInstance(
				cx.as_ptr(),
				constructor.handle().into(),
				object.handle().into(),
				&raw mut has_instance,
			)
		};
		if result { Ok(has_instance) } else { Err(Error::none()) }
	}
}

pub struct ClassObjectWrapper<T: ClassDefinition>(pub Box<T>);

impl<T: ClassDefinition> IntoValue<'_> for ClassObjectWrapper<T> {
	fn into_value(self: Box<Self>, cx: &Context, value: &mut Value) {
		T::new_object(cx, self.0).to_value(cx, value);
	}
}

trait SpecZero {
	fn is_zeroed(&self) -> bool;
}

impl SpecZero for JSFunctionSpec {
	fn is_zeroed(&self) -> bool {
		self.is_zeroed()
	}
}

impl SpecZero for JSPropertySpec {
	fn is_zeroed(&self) -> bool {
		self.is_zeroed()
	}
}

fn has_zero_spec<T: SpecZero>(specs: Option<&[T]>) -> bool {
	specs.and_then(|s| s.last()).is_none_or(T::is_zeroed)
}

fn unwrap_specs<T>(specs: Option<&[T]>) -> *const T {
	specs.map_or_else(ptr::null, <[T]>::as_ptr)
}

fn check_private<T: ClassDefinition>(cx: &Context, object: &Object) -> Result<()> {
	if unsafe { class_num_reserved_slots(get_object_class(object.handle().get())) >= 1 }
		&& (T::instance_of(cx, object) || T::has_instance(cx, object)?)
	{
		Ok(())
	} else {
		let name = unsafe { CStr::from_ptr(T::class().base.name).to_str()? };
		Err(Error::new(
			format!("Object does not implement interface {name}"),
			ErrorKind::Type,
		))
	}
}

unsafe extern "C" fn illegal_constructor(cx: *mut JSContext, _: u32, _: *mut JSVal) -> bool {
	unsafe {
		throw_type_error(cx, "Illegal constructor.");
	}
	false
}

#[doc(hidden)]
pub fn construct_native_object<'cx, T, C>(cx: &'cx Context, args: &mut Arguments<'cx>, constructor: C) -> bool
where
	T: ClassDefinition,
	C: for<'cx2> FnOnce(&'cx2 Context, &mut Arguments<'cx2>, &mut Object<'cx2>) -> ResultExc<()>,
{
	if !args.is_constructing() {
		debug_assert!(!Exception::is_pending(cx));
		unsafe {
			let name = CStr::from_ptr(T::class().base.name).to_str().unwrap();
			throw_type_error(cx.as_ptr(), &format!("{name} constructor: 'new' is required"));
		}
		return false;
	}

	let class_info = T::init_class(cx, &Object::global(cx)).1;

	let mut this = Object::from(cx.root(unsafe {
		JS_NewObjectWithGivenProto(cx.as_ptr(), &raw const T::class().base, class_info.prototype.handle())
	}));

	let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| constructor(cx, args, &mut this)));

	__handle_native_constructor_result(cx, result, &this, &mut args.rval())
}

#[doc(hidden)]
pub unsafe extern "C" fn finalise_native_object_operation<T>(_: *mut GCContext, this: *mut JSObject) {
	let mut value = NullValue();
	unsafe {
		JS_GetReservedSlot(this, 0, &raw mut value);
	}
	if value.is_double() && value.asBits_ & 0xFFFF000000000000 == 0 {
		let private = value.to_private().cast_mut().cast::<T>();
		let _ = unsafe { Box::from_raw(private) };
	}
}

#[doc(hidden)]
pub unsafe extern "C" fn trace_native_object_operation<T: Traceable>(trc: *mut JSTracer, this: *mut JSObject) {
	let mut value = NullValue();
	unsafe {
		JS_GetReservedSlot(this, 0, &raw mut value);
	}
	if value.is_double() && value.asBits_ & 0xFFFF000000000000 == 0 {
		unsafe {
			let private = &*(value.to_private().cast::<T>());
			private.trace(trc);
		}
	}
}
