/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::marker::PhantomPinned;

use mozjs::gc::{GCMethods, RootKind, RootedTraceableSet, Traceable};
use mozjs::jsapi::Heap;

use crate::Local;

#[derive(Debug)]
pub struct TracedHeap<T: GCMethods + Copy + 'static>
where
	Heap<T>: Traceable,
{
	heap: Box<Heap<T>>,
	_pin: PhantomPinned,
}

impl<T: GCMethods + Copy + 'static> TracedHeap<T>
where
	Heap<T>: Traceable + Default,
{
	pub fn new(value: T) -> TracedHeap<T> {
		let heap = Heap::boxed(value);
		unsafe { RootedTraceableSet::add(&*heap) };
		TracedHeap { heap, _pin: PhantomPinned }
	}
}

impl<T: GCMethods + Copy + 'static> TracedHeap<T>
where
	Heap<T>: Traceable,
{
	pub fn get(&self) -> T {
		self.heap.get()
	}

	pub fn set(&self, value: T) {
		self.heap.set(value);
	}
}

impl<T: GCMethods + RootKind + Copy + 'static> TracedHeap<T>
where
	Heap<T>: Traceable,
{
	pub fn to_local(&self) -> Local<'_, T> {
		unsafe { Local::from_heap(&self.heap) }
	}
}

impl<T: GCMethods + Copy + 'static> Drop for TracedHeap<T>
where
	Heap<T>: Traceable,
{
	fn drop(&mut self) {
		unsafe { RootedTraceableSet::remove(&*self.heap) }
	}
}
