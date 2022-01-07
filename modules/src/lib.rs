/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

#[macro_use]
extern crate ion;
#[macro_use]
extern crate mozjs;

use ion::IonContext;
use ion::objects::object::IonObject;
use runtime::modules::{init_global_module, init_module, StandardModules};

pub use crate::assert::Assert;
pub use crate::fs::FileSystem;
pub use crate::path::PathM;
pub use crate::url::UrlM;

mod assert;
mod fs;
mod path;
mod url;

#[derive(Default)]
pub struct Modules;

impl StandardModules for Modules {
	fn init(cx: IonContext, global: &mut IonObject) -> bool {
		unsafe {
			init_module::<Assert>(cx, global)
				&& init_module::<FileSystem>(cx, global)
				&& init_module::<PathM>(cx, global)
				&& init_module::<UrlM>(cx, global)
		}
	}

	fn init_globals(cx: IonContext, global: &mut IonObject) -> bool {
		unsafe {
			init_global_module::<Assert>(cx, global)
				&& init_global_module::<FileSystem>(cx, global)
				&& init_global_module::<PathM>(cx, global)
				&& init_global_module::<UrlM>(cx, global)
		}
	}
}
