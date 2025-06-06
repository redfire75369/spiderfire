/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::collections::HashMap;
use std::ptr;

use ion::module::{ModuleLoader, init_module_loader};
use ion::object::default_new_global;
use ion::{Context, ContextInner, ErrorReport, Object};
use mozjs::gc::Traceable;
use mozjs::glue::CreateJobQueue;
use mozjs::jsapi::{
	ContextOptionsRef, Heap, JSAutoRealm, JSObject, JSTracer, SetJobQueue, SetPromiseRejectionTrackerCallback,
};
use uuid::Uuid;

use crate::event_loop::future::FutureQueue;
use crate::event_loop::macrotasks::MacrotaskQueue;
use crate::event_loop::microtasks::{JOB_QUEUE_TRAPS, MicrotaskQueue};
use crate::event_loop::{EventLoop, promise_rejection_tracker_callback};
use crate::globals::{init_globals, init_microtasks, init_timers};
use crate::module::StandardModules;

#[derive(Default)]
pub struct ContextPrivate {
	pub(crate) event_loop: EventLoop,
	pub(crate) blob_store: HashMap<Uuid, Box<Heap<*mut JSObject>>>,
}

unsafe impl Traceable for ContextPrivate {
	unsafe fn trace(&self, trc: *mut JSTracer) {
		self.blob_store.values().for_each(|blob| unsafe { blob.trace(trc) });
	}
}

pub trait ContextExt {
	#[expect(clippy::mut_from_ref)]
	unsafe fn get_private(&self) -> &mut ContextPrivate;
}

impl ContextExt for Context {
	unsafe fn get_private(&self) -> &mut ContextPrivate {
		unsafe { (*self.get_raw_private()).as_any_mut().downcast_mut().unwrap() }
	}
}

pub struct Runtime<'cx> {
	global: Object<'cx>,
	cx: &'cx Context,
	#[expect(dead_code)]
	realm: JSAutoRealm,
}

impl<'cx> Runtime<'cx> {
	pub fn cx(&self) -> &Context {
		self.cx
	}

	pub fn global(&self) -> &Object<'cx> {
		&self.global
	}

	pub fn global_mut(&mut self) -> &Object<'cx> {
		&mut self.global
	}

	pub async fn run_event_loop(&self) -> Result<(), Option<ErrorReport>> {
		let event_loop = unsafe { &mut self.cx.get_private().event_loop };
		event_loop.run_event_loop(self.cx).await
	}
}

impl Drop for Runtime<'_> {
	fn drop(&mut self) {
		let inner_private = self.cx.get_inner_data().as_ptr();
		unsafe {
			let _ = Box::from_raw(inner_private);
			ContextInner::remove_tracer(self.cx.as_ptr(), inner_private);
		}
	}
}

#[derive(Copy, Clone, Debug)]
pub struct RuntimeBuilder<ML: ModuleLoader + 'static = (), Std: StandardModules + 'static = ()> {
	microtask_queue: bool,
	macrotask_queue: bool,
	modules: Option<ML>,
	standard_modules: Option<Std>,
}

impl<ML: ModuleLoader + 'static, Std: StandardModules + 'static> RuntimeBuilder<ML, Std> {
	pub fn new() -> RuntimeBuilder<ML, Std> {
		RuntimeBuilder::default()
	}

	pub fn macrotask_queue(mut self) -> RuntimeBuilder<ML, Std> {
		self.macrotask_queue = true;
		self
	}

	pub fn microtask_queue(mut self) -> RuntimeBuilder<ML, Std> {
		self.microtask_queue = true;
		self
	}

	pub fn modules(mut self, loader: ML) -> RuntimeBuilder<ML, Std> {
		self.modules = Some(loader);
		self
	}

	pub fn standard_modules(mut self, standard_modules: Std) -> RuntimeBuilder<ML, Std> {
		self.standard_modules = Some(standard_modules);
		self
	}

	pub fn build(self, cx: &mut Context) -> Runtime {
		let global = default_new_global(cx);
		let realm = JSAutoRealm::new(cx.as_ptr(), global.handle().get());

		let global_obj = global.handle().get();
		global.set_as(cx, "global", &global_obj);
		init_globals(cx, &global);

		let mut private = Box::<ContextPrivate>::default();

		if self.microtask_queue {
			private.event_loop.microtasks = Some(MicrotaskQueue::default());
			init_microtasks(cx, &global);
			private.event_loop.futures = Some(FutureQueue::default());

			unsafe {
				SetJobQueue(
					cx.as_ptr(),
					CreateJobQueue(
						&JOB_QUEUE_TRAPS,
						ptr::from_ref(private.event_loop.microtasks.as_ref().unwrap()).cast(),
					),
				);
				SetPromiseRejectionTrackerCallback(
					cx.as_ptr(),
					Some(promise_rejection_tracker_callback),
					ptr::null_mut(),
				);
			}
		}
		if self.macrotask_queue {
			private.event_loop.macrotasks = Some(MacrotaskQueue::default());
			init_timers(cx, &global);
		}

		let _options = unsafe { &mut *ContextOptionsRef(cx.as_ptr()) };

		cx.set_private(private);

		let has_loader = self.modules.is_some();
		if let Some(loader) = self.modules {
			init_module_loader(cx, loader);
		}

		if let Some(standard_modules) = self.standard_modules {
			if has_loader {
				standard_modules.init(cx, &global);
			} else {
				standard_modules.init_globals(cx, &global);
			}
		}

		Runtime { global, cx, realm }
	}
}

impl<ML: ModuleLoader + 'static, Std: StandardModules + 'static> Default for RuntimeBuilder<ML, Std> {
	fn default() -> RuntimeBuilder<ML, Std> {
		RuntimeBuilder {
			microtask_queue: false,
			macrotask_queue: false,
			modules: None,
			standard_modules: None,
		}
	}
}
