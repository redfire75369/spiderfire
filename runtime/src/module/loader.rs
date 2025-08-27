/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::collections::hash_map::{Entry, HashMap};
use std::ffi::OsStr;
use std::fs::read_to_string;
use std::path::{Path, PathBuf};

use dunce::canonicalize;
use ion::module::{Module, ModuleData, ModuleLoader, ModuleRequest, ModuleType};
use ion::{Context, Error, Local, Object, Result, Value};
use mozjs::jsapi::JSObject;
use url::Url;

use crate::cache::locate_in_cache;
use crate::cache::map::save_sourcemap;
use crate::config::Config;

#[derive(Debug, Eq, Hash, PartialEq)]
pub enum Specifier {
	Path(PathBuf),
}

#[derive(Debug, Eq, Hash, PartialEq)]
pub struct ModuleKey {
	pub(crate) specifier: Specifier,
	pub(crate) kind: ModuleType,
}

#[derive(Default)]
pub struct Loader {
	modules: HashMap<ModuleKey, *mut JSObject>,
}

impl Loader {
	fn resolve_specifier(specifier: &str, data: Option<&ModuleData>) -> Specifier {
		let path = if specifier.starts_with("./") || specifier.starts_with("../") {
			Path::new(data.and_then(|d| d.path.as_ref()).unwrap()).parent().unwrap().join(specifier)
		} else {
			Path::new(specifier).to_path_buf()
		};
		Specifier::Path(path)
	}
}

impl ModuleLoader for Loader {
	fn resolve<'cx>(&mut self, cx: &'cx Context, private: &Value, request: &ModuleRequest) -> Result<Module<'cx>> {
		let specifier = request.specifier(cx).to_owned(cx)?;
		let data = ModuleData::from_private(cx, private);

		let specifier = Loader::resolve_specifier(&specifier, data.as_ref());
		let kind = request.kind(cx);

		let key = ModuleKey { specifier, kind };
		let Specifier::Path(path) = &key.specifier;
		let name = path.to_str().unwrap();

		if let Some(module) = self.modules.get(&key) {
			return Ok(Module(Object::from(unsafe { Local::from_marked(module) })));
		}

		let script = read_to_string(path).map_err(|_| Error::new(format!("Unable to read module: {key:?}"), None))?;

		let module = match kind {
			ModuleType::JavaScript => {
				let is_typescript = Config::global().typescript && path.extension() == Some(OsStr::new("ts"));
				let (script, sourcemap) = is_typescript
					.then(|| locate_in_cache(path, &script))
					.flatten()
					.map_or_else(|| (script, None), |(s, sm)| (s, Some(sm)));
				if let Some(sourcemap) = sourcemap {
					save_sourcemap(path, sourcemap);
				}

				Module::compile_and_evaluate(cx, name, Some(path.as_path()), &script).map(|(module, _)| module)
			}
			ModuleType::Json => {
				Module::compile_and_evaluate(cx, name, Some(path.as_path()), &script).map(|(module, _)| module)
			}
		};

		match module {
			Ok(module) => {
				let request = ModuleRequest::new(cx, name, kind);
				self.register(cx, module.0.handle().get(), &request)?;
				Ok(module)
			}
			Err(_) => Err(Error::new(format!("Unable to compile module: {key:?}"), None)),
		}
	}

	fn register(&mut self, cx: &Context, module: *mut JSObject, request: &ModuleRequest) -> Result<()> {
		let specifier = Specifier::Path(PathBuf::from(request.specifier(cx).to_owned(cx)?));
		let kind = ModuleType::JavaScript;
		let key = ModuleKey { specifier, kind };
		match self.modules.entry(key) {
			Entry::Vacant(v) => {
				v.insert(module);
				Ok(())
			}
			Entry::Occupied(_) => Err(Error::new("Module already exists", None)),
		}
	}

	fn metadata(&self, cx: &Context, private: &Value, meta: &Object) -> Result<()> {
		let data = ModuleData::from_private(cx, private);

		if let Some(data) = data
			&& let Some(path) = data.path.as_ref()
		{
			let url = Url::from_file_path(canonicalize(path)?).unwrap();
			if !meta.set_as(cx, "url", url.as_str()) {
				return Err(Error::none());
			}
		}
		Ok(())
	}
}
