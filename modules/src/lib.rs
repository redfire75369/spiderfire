/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

#![allow(clippy::module_inception)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

#[macro_use]
extern crate ion;

use ion::{Context, Object};
use runtime::module::{StandardModules, init_global_module, init_module};

pub use crate::assert::Assert;
pub use crate::fs::{FileSystem, FileSystemSync};
pub use crate::path::PathM;
pub use crate::url::UrlM;

mod assert;
mod fs;
mod path;
mod url;

macro_rules! inner_init {
	($cx:ident, $global:ident, $init:ident) => {{
		fn inner(cx: &Context, global: &Object) -> Option<()> {
			$init(cx, global, &Assert)?;
			let fs_sync = $init(cx, global, &FileSystemSync)?;
			$init(cx, global, &FileSystem { sync: &fs_sync })?;
			$init(cx, global, &PathM)?;
			$init(cx, global, &UrlM)?;
			Some(())
		}
		inner($cx, $global).is_some()
	}};
}

pub struct Modules;

impl StandardModules for Modules {
	fn init(self, cx: &Context, global: &Object) -> bool {
		inner_init!(cx, global, init_module)
	}

	fn init_globals(self, cx: &Context, global: &Object) -> bool {
		inner_init!(cx, global, init_global_module)
	}
}
