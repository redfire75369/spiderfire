/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::path::Path;
use std::ptr;

use mozjs::jsapi::{
	CompileJsonModule, CompileModule, CreateModuleRequest, GetModuleRequestSpecifier, Handle, JS_GetRuntime, JSContext,
	JSObject, ModuleEvaluate, ModuleIsLinked, ModuleLink, SetModuleMetadataHook, SetModulePrivate,
	SetModuleResolveHook,
};
use mozjs::jsval::JSVal;
use mozjs::rust::{CompileOptionsWrapper, transform_u16_to_source_text};

use crate::conversions::{FromValue as _, ToValue as _};
use crate::{Context, Error, ErrorReport, Local, Object, Promise, ThrowException as _, Value};

/// Represents private module data
#[derive(Clone, Debug)]
pub struct ModuleData {
	pub path: Option<String>,
}

impl ModuleData {
	/// Creates [ModuleData] based on the private data of a module
	pub fn from_private(cx: &Context, private: &Value) -> Option<ModuleData> {
		private.handle().is_object().then(|| {
			let private = private.to_object(cx);
			let path: Option<String> = private.get_as(cx, "path", true, ()).unwrap();
			ModuleData { path }
		})
	}

	/// Converts [ModuleData] to an [Object] for storage.
	pub fn to_object<'cx>(&self, cx: &'cx Context) -> Object<'cx> {
		let object = Object::new(cx);
		object.set_as(cx, "path", &self.path);
		object
	}
}

#[derive(Copy, Clone, Debug, Default, Eq, Hash, PartialEq)]
pub enum ModuleType {
	#[default]
	JavaScript,
	Json,
}

/// Represents a request by the runtime for a module.
#[derive(Debug)]
pub struct ModuleRequest<'r>(Object<'r>);

impl<'r> ModuleRequest<'r> {
	/// Creates a new [ModuleRequest] with a given specifier.
	pub fn new<S: AsRef<str>>(cx: &'r Context, specifier: S) -> ModuleRequest<'r> {
		let specifier = crate::String::copy_from_str(cx, specifier.as_ref()).unwrap();
		ModuleRequest(cx.root(unsafe { CreateModuleRequest(cx.as_ptr(), specifier.handle().into()) }).into())
	}

	/// Creates a new [ModuleRequest] from a raw handle.
	///
	/// ### Safety
	/// `request` must be a valid module request object.
	pub unsafe fn from_raw_request(request: Handle<*mut JSObject>) -> ModuleRequest<'r> {
		ModuleRequest(Object::from(unsafe { Local::from_raw_handle(request) }))
	}

	/// Returns the specifier of the request.
	pub fn specifier<'cx>(&self, cx: &'cx Context) -> crate::String<'cx> {
		cx.root(unsafe { GetModuleRequestSpecifier(cx.as_ptr(), self.0.handle().into()) }).into()
	}

	pub fn kind(&self, _cx: &Context) -> ModuleType {
		// TODO: Use `GetModuleRequestType`
		ModuleType::JavaScript
	}
}

/// Represents phases of running modules.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModuleErrorKind {
	Compilation,
	Instantiation,
	Evaluation,
}

/// Represents errors that may occur when running modules.
#[derive(Clone, Debug)]
pub struct ModuleError {
	pub kind: ModuleErrorKind,
	pub report: ErrorReport,
}

impl ModuleError {
	/// Creates a [ModuleError] with a given report and phase.
	fn new(report: ErrorReport, kind: ModuleErrorKind) -> ModuleError {
		ModuleError { kind, report }
	}

	/// Formats the [ModuleError] for printing.
	pub fn format(&self, cx: &Context) -> String {
		self.report.format(cx)
	}
}

/// Represents a compiled module.
#[derive(Debug)]
pub struct Module<'m>(pub Object<'m>);

impl<'cx> Module<'cx> {
	/// Compiles a [Module] with the given source and filename.
	#[expect(clippy::result_large_err)]
	pub fn compile(
		cx: &'cx Context, filename: &str, path: Option<&Path>, script: &str, kind: ModuleType,
	) -> Result<Module<'cx>, ModuleError> {
		let script: Vec<_> = script.encode_utf16().collect();
		let mut source = transform_u16_to_source_text(script.as_slice());
		let filename = path.and_then(Path::to_str).unwrap_or(filename);
		let options = unsafe { CompileOptionsWrapper::new(cx.as_ptr(), filename, 1) };
		unsafe {
			(*options.ptr)._base.prefableOptions_.set_importAttributes_(true);
		}

		let module = match kind {
			ModuleType::JavaScript => unsafe { CompileModule(cx.as_ptr(), options.ptr.cast_const(), &mut source) },
			ModuleType::Json => unsafe { CompileJsonModule(cx.as_ptr(), options.ptr.cast_const(), &mut source) },
		};

		if !module.is_null() {
			let module = Module(Object::from(cx.root(module)));

			let data = ModuleData {
				path: path.and_then(Path::to_str).map(String::from),
			};

			unsafe {
				let private = data.to_object(cx).as_value(cx);
				SetModulePrivate(module.0.handle().get(), &*private.handle());
			}

			Ok(module)
		} else {
			Err(ModuleError::new(
				ErrorReport::new(cx).unwrap().unwrap(),
				ModuleErrorKind::Compilation,
			))
		}
	}

	/// Compiles and evaluates a [Module] with the given source and filename.
	/// On success, returns the compiled module object and a promise. The promise resolves with the return value of the module.
	/// The promise is a byproduct of enabling top-level await.
	#[expect(clippy::result_large_err)]
	pub fn compile_and_evaluate(
		cx: &'cx Context, filename: &str, path: Option<&Path>, script: &str,
	) -> Result<(Module<'cx>, Option<Promise<'cx>>), ModuleError> {
		let module = Module::compile(cx, filename, path, script, ModuleType::JavaScript)?;

		if let Err(error) = module.link(cx) {
			return Err(ModuleError::new(error, ModuleErrorKind::Instantiation));
		}

		match module.evaluate(cx) {
			Ok(val) => {
				let promise = Promise::from_value(cx, &val, true, ()).ok();
				Ok((module, promise))
			}
			Err(error) => Err(ModuleError::new(error, ModuleErrorKind::Evaluation)),
		}
	}

	/// Links a [Module]. Generally called by [Module::compile_and_evaluate].
	pub fn link(&self, cx: &Context) -> Result<(), ErrorReport> {
		if unsafe { ModuleLink(cx.as_ptr(), self.0.handle().into()) } {
			Ok(())
		} else {
			Err(ErrorReport::new(cx)?.unwrap())
		}
	}

	/// Evaluates a [Module]. Generally called by [Module::compile].
	pub fn evaluate(&self, cx: &'cx Context) -> Result<Value<'cx>, ErrorReport> {
		let mut rval = Value::undefined(cx);
		if unsafe { ModuleEvaluate(cx.as_ptr(), self.0.handle().into(), rval.handle_mut().into()) } {
			Ok(rval)
		} else {
			Err(ErrorReport::new_with_exception_stack(cx)?.unwrap())
		}
	}

	/// Returns `true` if the module has been linked.
	pub fn is_linked(&self) -> bool {
		unsafe { ModuleIsLinked(self.0.handle().get()) }
	}
}

/// Represents an ES module loader.
pub trait ModuleLoader {
	/// Given a request and private data of a module, resolves the request into a compiled module object.
	/// Should return the same module object for a given request.
	fn resolve<'cx>(
		&mut self, cx: &'cx Context, private: &Value, request: &ModuleRequest,
	) -> crate::Result<Module<'cx>>;

	/// Registers a new module in the module registry. Useful for native modules.
	fn register(&mut self, cx: &Context, module: *mut JSObject, request: &ModuleRequest) -> crate::Result<()>;

	/// Returns metadata of a module, used to populate `import.meta`.
	fn metadata(&self, cx: &Context, private: &Value, meta: &Object) -> crate::Result<()>;
}

impl ModuleLoader for () {
	fn resolve<'cx>(&mut self, _: &'cx Context, _: &Value, _: &ModuleRequest) -> crate::Result<Module<'cx>> {
		Err(Error::new("Modules are unsupported by this loader.", None))
	}

	fn register(&mut self, _: &Context, _: *mut JSObject, _: &ModuleRequest) -> crate::Result<()> {
		Ok(())
	}

	fn metadata(&self, _: &Context, _: &Value, _: &Object) -> crate::Result<()> {
		Ok(())
	}
}

/// Initialises a module loader in the current runtime.
pub fn init_module_loader<ML: ModuleLoader + 'static>(cx: &Context, loader: ML) {
	unsafe extern "C" fn resolve(
		cx: *mut JSContext, private: Handle<JSVal>, request: Handle<*mut JSObject>,
	) -> *mut JSObject {
		let cx = &unsafe { Context::new_unchecked(cx) };

		let loader = unsafe { &mut (*cx.get_inner_data().as_ptr()).module_loader };
		loader
			.as_mut()
			.and_then(|loader| {
				let private = unsafe { Value::from(Local::from_raw_handle(private)) };
				let request = unsafe { ModuleRequest::from_raw_request(request) };
				match loader.resolve(cx, &private, &request) {
					Ok(module) => Some(module.0.handle().get()),
					Err(e) => {
						e.throw(cx);
						None
					}
				}
			})
			.unwrap_or_else(ptr::null_mut)
	}

	unsafe extern "C" fn metadata(
		cx: *mut JSContext, private_data: Handle<JSVal>, metadata: Handle<*mut JSObject>,
	) -> bool {
		let cx = &unsafe { Context::new_unchecked(cx) };

		let loader = unsafe { &mut (*cx.get_inner_data().as_ptr()).module_loader };
		loader.as_mut().is_none_or(|loader| {
			let private = Value::from(unsafe { Local::from_raw_handle(private_data) });
			let metadata = Object::from(unsafe { Local::from_raw_handle(metadata) });
			match loader.metadata(cx, &private, &metadata) {
				Ok(_) => true,
				Err(e) => {
					e.throw(cx);
					false
				}
			}
		})
	}

	unsafe {
		(*cx.get_inner_data().as_ptr()).module_loader = Some(Box::new(loader));

		let rt = JS_GetRuntime(cx.as_ptr());
		SetModuleResolveHook(rt, Some(resolve));
		SetModuleMetadataHook(rt, Some(metadata));
	}
}
