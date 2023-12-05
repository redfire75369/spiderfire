/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::path::Path;
use std::ptr;

use mozjs::jsapi::{
	CompileModule, CreateModuleRequest, GetModuleRequestSpecifier, Handle, JS_GetRuntime, JSContext, JSObject,
	ModuleEvaluate, ModuleLink, SetModuleMetadataHook, SetModulePrivate, SetModuleResolveHook,
};
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rust::{CompileOptionsWrapper, transform_u16_to_source_text};

use crate::{Context, ErrorReport, Exception, Object, Promise, Value};
use crate::conversions::{FromValue, ToValue};

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
			let path = private.get_as(cx, "path", true, ()).and_then(|r| r.ok());
			ModuleData { path }
		})
	}

	/// Converts [ModuleData] to an [Object] for storage.
	pub fn to_object(&self, cx: &Context) -> Object {
		let mut object = Object::new(cx);
		let _ = object.set_as(cx, "path", &self.path);
		object
	}
}

/// Represents a request by the runtime for a module.
#[derive(Debug)]
pub struct ModuleRequest(Object);

impl ModuleRequest {
	/// Creates a new [ModuleRequest] with a given specifier.
	pub fn new<S: AsRef<str>>(cx: &Context, specifier: S) -> ModuleRequest {
		let specifier = crate::String::copy_from_str(cx, specifier.as_ref()).unwrap();
		ModuleRequest(cx.root(unsafe { CreateModuleRequest(cx.as_ptr(), specifier.handle().into()) }).into())
	}

	/// Creates a new [ModuleRequest] from a raw handle.
	///
	/// ### Safety
	/// `request` must be a valid module request object.
	pub unsafe fn from_request(cx: &Context, request: Handle<*mut JSObject>) -> ModuleRequest {
		ModuleRequest(Object::from(cx.root(request.get())))
	}

	/// Returns the specifier of the request.
	pub fn specifier(&self, cx: &Context) -> crate::String {
		cx.root(unsafe { GetModuleRequestSpecifier(cx.as_ptr(), self.0.handle().into()) }).into()
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
pub struct Module(pub Object);

impl Module {
	/// Compiles a [Module] with the given source and filename.
	/// On success, returns the compiled module object and a promise. The promise resolves with the return value of the module.
	/// The promise is a byproduct of enabling top-level await.
	#[allow(clippy::result_large_err)]
	pub fn compile(
		cx: &Context, filename: &str, path: Option<&Path>, script: &str,
	) -> Result<(Module, Option<Promise>), ModuleError> {
		let script: Vec<u16> = script.encode_utf16().collect();
		let mut source = transform_u16_to_source_text(script.as_slice());
		let filename = path.and_then(Path::to_str).unwrap_or(filename);
		let options = unsafe { CompileOptionsWrapper::new(cx.as_ptr(), filename, 1) };

		let module = unsafe { CompileModule(cx.as_ptr(), options.ptr.cast_const().cast(), &mut source) };

		if !module.is_null() {
			let module = Module(Object::from(cx.root(module)));

			let data = ModuleData {
				path: path.and_then(Path::to_str).map(String::from),
			};

			match data.to_object(cx).to_value(cx) {
				Ok(private) => unsafe { SetModulePrivate(module.0.handle().get(), &*private.handle()) },
				Err(error) => {
					return Err(ModuleError::new(
						ErrorReport::from(Exception::Error(error), None),
						ModuleErrorKind::Instantiation,
					));
				}
			}

			if let Err(error) = module.instantiate(cx) {
				return Err(ModuleError::new(error, ModuleErrorKind::Instantiation));
			}

			let eval_result = module.evaluate(cx);
			match eval_result {
				Ok(val) => {
					let promise = Promise::from_value(cx, &val, true, ()).ok();
					Ok((module, promise))
				}
				Err(error) => Err(ModuleError::new(error, ModuleErrorKind::Evaluation)),
			}
		} else {
			Err(ModuleError::new(
				ErrorReport::new(cx).unwrap(),
				ModuleErrorKind::Compilation,
			))
		}
	}

	/// Instantiates a [Module]. Generally called by [Module::compile].
	pub fn instantiate(&self, cx: &Context) -> Result<(), ErrorReport> {
		if unsafe { ModuleLink(cx.as_ptr(), self.0.handle().into()) } {
			Ok(())
		} else {
			Err(ErrorReport::new(cx).unwrap())
		}
	}

	/// Evaluates a [Module]. Generally called by [Module::compile].
	pub fn evaluate(&self, cx: &Context) -> Result<Value, ErrorReport> {
		rooted!(in(cx.as_ptr()) let mut rval = UndefinedValue());
		if unsafe { ModuleEvaluate(cx.as_ptr(), self.0.handle().into(), rval.handle_mut().into()) } {
			Ok(Value::from(cx.root(rval.get())))
		} else {
			Err(ErrorReport::new_with_exception_stack(cx).unwrap())
		}
	}
}

/// Represents an ES module loader.
pub trait ModuleLoader {
	/// Given a request and private data of a module, resolves the request into a compiled module object.
	/// Should return the same module object for a given request.
	fn resolve(&mut self, cx: &Context, private: &Value, request: &ModuleRequest) -> *mut JSObject;

	/// Registers a new module in the module registry. Useful for native modules.
	fn register(&mut self, cx: &Context, module: *mut JSObject, request: &ModuleRequest) -> *mut JSObject;

	/// Returns metadata of a module, used to populate `import.meta`.
	fn metadata(&self, cx: &Context, private: &Value, meta: &mut Object) -> bool;
}

impl ModuleLoader for () {
	fn resolve(&mut self, _: &Context, _: &Value, _: &ModuleRequest) -> *mut JSObject {
		ptr::null_mut()
	}

	fn register(&mut self, _: &Context, _: *mut JSObject, _: &ModuleRequest) -> *mut JSObject {
		ptr::null_mut()
	}

	fn metadata(&self, _: &Context, _: &Value, _: &mut Object) -> bool {
		true
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
			.map(|loader| {
				let private = Value::from(cx.root(private.get()));
				let request = unsafe { ModuleRequest::from_request(cx, request) };
				loader.resolve(cx, &private, &request)
			})
			.unwrap_or_else(ptr::null_mut)
	}

	unsafe extern "C" fn metadata(
		cx: *mut JSContext, private_data: Handle<JSVal>, metadata: Handle<*mut JSObject>,
	) -> bool {
		let cx = &unsafe { Context::new_unchecked(cx) };

		let loader = unsafe { &mut (*cx.get_inner_data().as_ptr()).module_loader };
		loader
			.as_mut()
			.map(|loader| {
				let private = Value::from(cx.root(private_data.get()));
				let mut metadata = Object::from(cx.root(metadata.get()));
				loader.metadata(cx, &private, &mut metadata)
			})
			.unwrap_or_else(|| true)
	}

	unsafe {
		(*cx.get_inner_data().as_ptr()).module_loader = Some(Box::new(loader));

		let rt = JS_GetRuntime(cx.as_ptr());
		SetModuleResolveHook(rt, Some(resolve));
		SetModuleMetadataHook(rt, Some(metadata));
	}
}
