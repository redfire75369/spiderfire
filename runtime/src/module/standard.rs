/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use ion::flags::PropertyFlags;
use ion::module::{Module, ModuleRequest, ModuleType};
use ion::{Context, Object};

pub trait StandardModules {
	fn init(self, cx: &Context, global: &Object) -> bool;

	fn init_globals(self, cx: &Context, global: &Object) -> bool;
}

impl StandardModules for () {
	fn init(self, _: &Context, _: &Object) -> bool {
		true
	}

	fn init_globals(self, _: &Context, _: &Object) -> bool {
		true
	}
}

pub trait NativeModule<'cx> {
	const NAME: &'static str;
	const VARIABLE_NAME: &'static str;
	const SOURCE: &'static str;

	fn module(&self, cx: &'cx Context) -> Option<Object<'cx>>;
}

impl<M: for<'cx> NativeModule<'cx> + 'static> StandardModules for M {
	fn init(self, cx: &Context, global: &Object) -> bool {
		init_module(cx, global, &self).is_some()
	}

	fn init_globals(self, cx: &Context, global: &Object) -> bool {
		init_global_module(cx, global, &self).is_some()
	}
}

// TODO: Remove JS Wrapper, Stop Global Scope Pollution, Use CreateEmptyModule and AddModuleExport
// TODO: Waiting on https://bugzilla.mozilla.org/show_bug.cgi?id=1722802
pub fn init_module<'cx, M: NativeModule<'cx>>(cx: &'cx Context, global: &Object, module: &M) -> Option<Object<'cx>> {
	let internal = format!("______{}Internal______", M::VARIABLE_NAME);

	if let Some(module) = module.module(cx)
		&& global.define_as(cx, internal, &module, PropertyFlags::CONSTANT)
	{
		let loader = unsafe { &mut (*cx.get_inner_data().as_ptr()).module_loader };
		return loader
			.as_mut()
			.is_some_and(|loader| {
				let module = Module::compile(cx, M::NAME, None, M::SOURCE, ModuleType::JavaScript).unwrap();
				let request = ModuleRequest::new(cx, M::NAME, ModuleType::JavaScript);
				loader.register(cx, module.0.handle().get(), &request).is_ok()
			})
			.then_some(module);
	}
	None
}

pub fn init_global_module<'cx, M: NativeModule<'cx>>(
	cx: &'cx Context, global: &Object, module: &M,
) -> Option<Object<'cx>> {
	let module = module.module(cx)?;
	global
		.define_as(cx, M::VARIABLE_NAME, &module, PropertyFlags::CONSTANT_ENUMERATED)
		.then_some(module)
}
