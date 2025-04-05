/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr;
use std::ptr::NonNull;

use mozjs::gc::Traceable;
use mozjs::jsapi::{
	JS_AddExtraGCRootsTracer, JS_GetContextPrivate, JS_RemoveExtraGCRootsTracer, JS_SetContextPrivate, JSContext,
	JSTracer, Rooted,
};
use mozjs::rust::Runtime;
use private::RootedArena;

use crate::Local;
use crate::class::ClassInfo;
use crate::module::ModuleLoader;

/// Represents Types that can be Rooted in SpiderMonkey
#[derive(Clone, Copy, Debug)]
pub enum GCType {
	Value,
	Object,
	String,
	Script,
	PropertyKey,
	PropertyDescriptor,
	Function,
	BigInt,
	Symbol,
}

pub trait TraceablePrivate: Traceable + Any {
	fn as_any(&self) -> &dyn Any;

	fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: Traceable + Any> TraceablePrivate for T {
	fn as_any(&self) -> &dyn Any {
		self
	}

	fn as_any_mut(&mut self) -> &mut dyn Any {
		self
	}
}

#[derive(Default)]
pub struct ContextInner {
	pub class_infos: HashMap<TypeId, ClassInfo>,
	pub module_loader: Option<Box<dyn ModuleLoader>>,
	private: Option<Box<dyn TraceablePrivate>>,
}

impl ContextInner {
	unsafe fn add_tracer(cx: *mut JSContext, inner: *mut ContextInner) {
		unsafe {
			JS_AddExtraGCRootsTracer(cx, Some(ContextInner::trace), inner.cast::<c_void>());
		}
	}

	pub unsafe fn remove_tracer(cx: *mut JSContext, inner: *mut ContextInner) {
		unsafe {
			JS_RemoveExtraGCRootsTracer(cx, Some(ContextInner::trace), inner.cast::<c_void>());
		}
	}

	extern "C" fn trace(trc: *mut JSTracer, data: *mut c_void) {
		unsafe {
			let inner = &mut *data.cast::<ContextInner>();
			for info in inner.class_infos.values() {
				info.trace(trc);
			}
			inner.private.trace(trc);
		}
	}
}

/// Represents the thread-local state of the runtime.
///
/// Wrapper around [JSContext] that provides lifetime information and convenient APIs.
pub struct Context {
	context: NonNull<JSContext>,
	rooted: RootedArena,
	order: RefCell<Vec<GCType>>,
	private: NonNull<ContextInner>,
}

impl Context {
	pub fn from_runtime(rt: &Runtime) -> Context {
		let cx = rt.cx();

		let private = NonNull::new(unsafe { JS_GetContextPrivate(cx).cast::<ContextInner>() }).unwrap_or_else(|| {
			let private = Box::<ContextInner>::default();
			let private = Box::into_raw(private);
			unsafe {
				JS_SetContextPrivate(cx, private.cast());
				ContextInner::add_tracer(cx, private);
			}
			unsafe { NonNull::new_unchecked(private) }
		});

		Context {
			context: unsafe { NonNull::new_unchecked(cx) },
			rooted: RootedArena::default(),
			order: RefCell::new(Vec::new()),
			private,
		}
	}

	pub unsafe fn new_unchecked(cx: *mut JSContext) -> Context {
		Context {
			context: unsafe { NonNull::new_unchecked(cx) },
			rooted: RootedArena::default(),
			order: RefCell::new(Vec::new()),
			private: unsafe { NonNull::new_unchecked(JS_GetContextPrivate(cx).cast::<ContextInner>()) },
		}
	}

	pub fn as_ptr(&self) -> *mut JSContext {
		self.context.as_ptr()
	}

	pub fn get_inner_data(&self) -> NonNull<ContextInner> {
		self.private
	}

	pub fn get_raw_private(&self) -> *mut dyn TraceablePrivate {
		let inner = self.get_inner_data();
		ptr::from_mut(unsafe { (*inner.as_ptr()).private.as_deref_mut().unwrap() })
	}

	pub fn set_private(&self, private: Box<dyn TraceablePrivate>) {
		let inner_private = self.get_inner_data();
		unsafe {
			(*inner_private.as_ptr()).private = Some(private);
		}
	}

	/// Roots a value and returns a `[Local]` to it.
	/// The Local is only unrooted when the `[Context]` is dropped
	pub fn root<T: Rootable>(&self, value: T) -> Local<T> {
		let root = T::alloc(&self.rooted, Rooted::new_unrooted(value));
		self.order.borrow_mut().push(T::GC_TYPE);
		Local::new(self, root)
	}
}

pub trait Rootable: private::Sealed {}

impl<T: private::Sealed> Rootable for T {}

mod private {
	use mozjs::gc::RootKind;
	use mozjs::jsapi::{
		BigInt, JSFunction, JSObject, JSScript, JSString, PropertyDescriptor, PropertyKey, Rooted, Symbol,
	};
	use mozjs::jsval::JSVal;
	use typed_arena::Arena;

	use super::GCType;

	/// Holds Rooted Values
	#[derive(Default)]
	pub struct RootedArena {
		pub values: Arena<Rooted<JSVal>>,
		pub objects: Arena<Rooted<*mut JSObject>>,
		pub strings: Arena<Rooted<*mut JSString>>,
		pub scripts: Arena<Rooted<*mut JSScript>>,
		pub property_keys: Arena<Rooted<PropertyKey>>,
		pub property_descriptors: Arena<Rooted<PropertyDescriptor>>,
		pub functions: Arena<Rooted<*mut JSFunction>>,
		pub big_ints: Arena<Rooted<*mut BigInt>>,
		pub symbols: Arena<Rooted<*mut Symbol>>,
	}

	#[expect(clippy::mut_from_ref)]
	pub trait Sealed: RootKind + Copy + Sized {
		const GC_TYPE: GCType;

		fn alloc(arena: &RootedArena, root: Rooted<Self>) -> &mut Rooted<Self>;
	}

	macro_rules! impl_rootable {
		($(($value:ty, $key:ident, $gc_type:ident)$(,)?)*) => {
			$(
				impl Sealed for $value {
					const GC_TYPE: GCType = GCType::$gc_type;

					fn alloc(arena: &RootedArena, root: Rooted<$value>) -> &mut Rooted<$value> {
						arena.$key.alloc(root)
					}
				}
			)*
		};
	}

	impl_rootable! {
		(JSVal, values, Value),
		(*mut JSObject, objects, Object),
		(*mut JSString, strings, String),
		(*mut JSScript, scripts, Script),
		(PropertyKey, property_keys, PropertyKey),
		(PropertyDescriptor, property_descriptors, PropertyDescriptor),
		(*mut JSFunction, functions, Function),
		(*mut BigInt, big_ints, BigInt),
		(*mut Symbol, symbols, Symbol),
	}
}

macro_rules! impl_drop {
	([$self:expr], $(($key:ident, $gc_type:ident)$(,)?)*) => {
		$(let $key: Vec<_> = $self.rooted.$key.iter_mut().collect();)*
		$(let mut $key = $key.into_iter().rev();)*

		for ty in $self.order.take().into_iter().rev() {
			match ty {
				$(
					GCType::$gc_type => {
						let root = $key.next().unwrap();
						unsafe {
							root.remove_from_root_stack();
						}
					}
				)*
			}
		}
	}
}

impl Drop for Context {
	/// Drops the rooted values in reverse-order to maintain LIFO destruction in the Linked List.
	fn drop(&mut self) {
		impl_drop! {
			[self],
			(values, Value),
			(objects, Object),
			(strings, String),
			(scripts, Script),
			(property_keys, PropertyKey),
			(property_descriptors, PropertyDescriptor),
			(functions, Function),
			(big_ints, BigInt),
			(symbols, Symbol),
		}
	}
}
