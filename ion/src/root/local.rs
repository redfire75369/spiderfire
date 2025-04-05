/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::fmt;
use std::fmt::{Debug, Formatter};

use mozjs::gc::{GCMethods, Handle, MutableHandle, RootKind};
use mozjs::jsapi::{Handle as RawHandle, Heap, MutableHandle as RawMutableHandle, Rooted};

use crate::Context;

/// Represents a local reference managed by the Garbage Collector.
/// Prevents a local value that is currently being used from being garbage collected and causing undefined behaviour.
pub enum Local<'local, T: 'local>
where
	T: RootKind,
{
	Rooted(&'local mut Rooted<T>),
	Mutable(MutableHandle<'local, T>),
	Handle(Handle<'local, T>),
}

impl<'local, T: Copy + RootKind> Local<'local, T> {
	/// Forms a [Handle] to the [Local] which can be passed to SpiderMonkey APIs.
	pub fn handle(&self) -> Handle<'local, T> {
		match self {
			Self::Rooted(root) => unsafe { Handle::from_marked_location(&root.data) },
			Self::Mutable(handle) => unsafe { Handle::from_marked_location(&handle.get()) },
			Self::Handle(handle) => *handle,
		}
	}

	/// Forms a [Handle] to the [Local] which can be passed to SpiderMonkey APIs.
	///
	/// ### Panics
	/// Panics when a [`Local::Handle`] is passed.
	pub fn handle_mut(&mut self) -> MutableHandle<'local, T> {
		match self {
			Local::Rooted(root) => unsafe { MutableHandle::from_marked_location(&mut root.data) },
			Local::Mutable(handle) => unsafe { MutableHandle::from_marked_location(handle.as_mut()) },
			Local::Handle(_) => panic!("&mut Local::Handle should never be constructed"),
		}
	}

	pub fn get(&self) -> T {
		match self {
			Self::Rooted(root) => root.data,
			Self::Mutable(handle) => handle.get(),
			Self::Handle(handle) => handle.get(),
		}
	}
}

impl<'local, T: RootKind> Local<'local, T> {
	/// Creates a new [Local].
	/// `Context::root` should be used instead.
	pub(crate) fn new(cx: &Context, root: &'local mut Rooted<T>) -> Local<'local, T> {
		unsafe {
			Rooted::add_to_root_stack(root, cx.as_ptr());
		}
		Local::Rooted(root)
	}

	/// Creates a [Local] from a [Handle].
	pub fn from_handle(handle: Handle<'local, T>) -> Local<'local, T> {
		Local::Handle(handle)
	}

	/// Creates a [Local] from a [MutableHandle].
	pub fn from_handle_mut(handle: MutableHandle<'local, T>) -> Local<'local, T> {
		Local::Mutable(handle)
	}

	/// Creates a [Local] from a marked (pointer).
	///
	/// ### Safety
	/// The pointer must point to a location marked by the Garbage Collector.
	pub unsafe fn from_marked(ptr: *const T) -> Local<'local, T> {
		Local::Handle(unsafe { Handle::from_marked_location(ptr) })
	}

	/// Creates a [Local] from a [raw handle](RawHandle).
	///
	/// ### Safety
	/// The handle must be valid and still be recognised by the Garbage Collector.
	pub unsafe fn from_raw_handle(handle: RawHandle<T>) -> Local<'local, T> {
		Local::Handle(unsafe { Handle::from_raw(handle) })
	}

	/// Creates a [Local] from a marked [mutable pointer](pointer);
	///
	/// ### Safety
	/// The pointer must point to a location marked by the Garbage Collector.
	pub unsafe fn from_marked_mut(ptr: *mut T) -> Local<'local, T> {
		Local::Mutable(unsafe { MutableHandle::from_marked_location(ptr) })
	}

	/// Creates a [Local] from a [raw mutable handle](RawMutableHandle);
	///
	/// ### Safety
	/// The mutable handle must be valid and still be recognised by the Garbage Collector.
	pub unsafe fn from_raw_handle_mut(handle: RawMutableHandle<T>) -> Local<'local, T> {
		Local::Mutable(unsafe { MutableHandle::from_raw(handle) })
	}
}

impl<'local, T: Copy + GCMethods + RootKind> Local<'local, T> {
	pub unsafe fn from_heap(heap: &'local Heap<T>) -> Local<'local, T> {
		unsafe { Local::from_raw_handle(heap.handle()) }
	}
}

impl<T: Copy + Debug + RootKind> Debug for Local<'_, T> {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		self.handle().fmt(f)
	}
}
