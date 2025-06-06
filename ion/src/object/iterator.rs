/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::{iter, ptr};

use mozjs::gc::Traceable;
use mozjs::jsapi::{
	GetRealmIteratorPrototype, Heap, JSCLASS_BACKGROUND_FINALIZE, JSClass, JSClassOps, JSContext, JSFunctionSpec,
	JSNativeWrapper, JSObject, JSTracer,
};
use mozjs::jsval::JSVal;

use crate::class::{
	NativeClass, NativeObject, PrototypeChain, Reflector, TypeIdWrapper, finalise_native_object_operation,
	trace_native_object_operation,
};
use crate::conversions::{IntoValue, ToValue};
use crate::flags::PropertyFlags;
use crate::function::NativeFunction;
use crate::object::class_reserved_slots;
use crate::spec::{create_function_spec, create_function_spec_symbol};
use crate::symbol::WellKnownSymbolCode;
use crate::{Arguments, ClassDefinition, Context, Local, Object, ThrowException as _, Value};

pub trait JSIterator {
	fn next_value<'cx>(&mut self, cx: &'cx Context, private: &Value<'cx>) -> Option<Value<'cx>>;
}

impl<T, I: iter::Iterator<Item = T>> JSIterator for I
where
	T: for<'cx> IntoValue<'cx>,
{
	fn next_value<'cx>(&mut self, cx: &'cx Context, _: &Value) -> Option<Value<'cx>> {
		self.next().map(|val| {
			let mut rval = Value::undefined(cx);
			Box::new(val).into_value(cx, &mut rval);
			rval
		})
	}
}

pub struct IteratorResult<'cx> {
	value: Value<'cx>,
	done: bool,
}

impl<'cx> ToValue<'cx> for IteratorResult<'cx> {
	fn to_value(&self, cx: &'cx Context, value: &mut Value) {
		let object = Object::new(cx);
		object.set_as(cx, "value", &self.value);
		object.set_as(cx, "done", &self.done);
		object.to_value(cx, value);
	}
}

pub struct Iterator {
	reflector: Reflector,
	iter: Box<dyn JSIterator>,
	private: Box<Heap<JSVal>>,
}

impl Iterator {
	pub fn new<I: JSIterator + 'static>(iter: I, private: &Value) -> Iterator {
		Iterator {
			reflector: Reflector::default(),
			iter: Box::new(iter),
			private: Heap::boxed(private.get()),
		}
	}

	pub fn next_value<'cx>(&'cx mut self, cx: &'cx Context) -> IteratorResult<'cx> {
		let private = Value::from(unsafe { Local::from_heap(&self.private) });
		let next = self.iter.next_value(cx, &private);
		IteratorResult {
			done: next.is_none(),
			value: next.unwrap_or_else(Value::undefined_handle),
		}
	}
}

impl Iterator {
	unsafe extern "C" fn next_raw(cx: *mut JSContext, argc: u32, vp: *mut JSVal) -> bool {
		let cx = &unsafe { Context::new_unchecked(cx) };
		let args = &mut unsafe { Arguments::new(cx, argc, vp) };

		let this = args.this().to_object(cx);
		let iterator = match Iterator::get_mut_private(cx, &this) {
			Ok(iterator) => iterator,
			Err(e) => {
				e.throw(cx);
				return false;
			}
		};
		let result = iterator.next_value(cx);

		result.to_value(cx, &mut args.rval());
		true
	}

	unsafe extern "C" fn iterable(cx: *mut JSContext, argc: u32, vp: *mut JSVal) -> bool {
		let cx = &unsafe { Context::new_unchecked(cx) };
		let args = &mut unsafe { Arguments::new(cx, argc, vp) };

		let this = args.this().handle().get();
		args.rval().handle_mut().set(this);

		true
	}
}

impl IntoValue<'_> for Iterator {
	fn into_value(self: Box<Self>, cx: &Context, value: &mut Value) {
		let object = cx.root(Iterator::new_object(cx, self));
		object.handle().get().to_value(cx, value);
	}
}

unsafe impl Traceable for Iterator {
	unsafe fn trace(&self, trc: *mut JSTracer) {
		unsafe {
			self.private.trace(trc);
		}
	}
}

static ITERATOR_CLASS_OPS: JSClassOps = JSClassOps {
	addProperty: None,
	delProperty: None,
	enumerate: None,
	newEnumerate: None,
	resolve: None,
	mayResolve: None,
	finalize: Some(finalise_native_object_operation::<Iterator>),
	call: None,
	construct: None,
	trace: Some(trace_native_object_operation::<Iterator>),
};

static ITERATOR_CLASS: NativeClass = NativeClass {
	base: JSClass {
		name: c"NativeIterator".as_ptr(),
		flags: JSCLASS_BACKGROUND_FINALIZE | class_reserved_slots(1),
		cOps: &ITERATOR_CLASS_OPS,
		spec: ptr::null_mut(),
		ext: ptr::null_mut(),
		oOps: ptr::null_mut(),
	},
	prototype_chain: PrototypeChain::new().push(&TypeIdWrapper::<Iterator>::new()),
};

static ITERATOR_METHODS: &[JSFunctionSpec] = &[
	create_function_spec(
		c"next",
		JSNativeWrapper {
			op: Some(Iterator::next_raw),
			info: ptr::null_mut(),
		},
		0,
		PropertyFlags::CONSTANT_ENUMERATED,
	),
	create_function_spec_symbol(
		WellKnownSymbolCode::Iterator,
		JSNativeWrapper {
			op: Some(Iterator::iterable),
			info: ptr::null_mut(),
		},
		0,
		PropertyFlags::CONSTANT,
	),
	JSFunctionSpec::ZERO,
];

impl NativeObject for Iterator {
	fn reflector(&self) -> &Reflector {
		&self.reflector
	}
}

impl ClassDefinition for Iterator {
	fn class() -> &'static NativeClass {
		&ITERATOR_CLASS
	}

	fn proto_class() -> Option<&'static NativeClass> {
		None
	}

	fn parent_prototype(cx: &Context) -> Option<Local<*mut JSObject>> {
		Some(cx.root(unsafe { GetRealmIteratorPrototype(cx.as_ptr()) }))
	}

	fn constructor() -> (Option<NativeFunction>, u32) {
		(None, 0)
	}

	fn functions() -> Option<&'static [JSFunctionSpec]> {
		Some(ITERATOR_METHODS)
	}
}
