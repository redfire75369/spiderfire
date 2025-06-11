/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::ffi::OsStr;
use std::fs::read_to_string;
use std::io::ErrorKind;
use std::path::Path;

use ion::Context;
use ion::format::{Config as FormatConfig, format_value};
use ion::module::Module;
use ion::script::Script;
use modules::Modules;
use mozjs::rust::{JSEngine, Runtime as RustRuntime};
use runtime::cache::locate_in_cache;
use runtime::cache::map::{save_sourcemap, transform_error_report_with_sourcemaps};
use runtime::config::Config;
use runtime::module::Loader;
use runtime::{Runtime, RuntimeBuilder};
use swc_sourcemap::SourceMap;

pub(crate) async fn eval_inline(rt: &Runtime<'_>, source: &str) {
	let result = Script::compile_and_evaluate(rt.cx(), Path::new("inline.js"), source);

	match result {
		Ok(v) => println!("{}", format_value(rt.cx(), FormatConfig::default().quoted(true), &v)),
		Err(report) => eprintln!("{}", report.format(rt.cx())),
	}
	run_event_loop(rt).await;
}

pub(crate) async fn eval_script(path: &Path) {
	let engine = JSEngine::init().unwrap();
	let rt = RustRuntime::new(engine.handle());

	let cx = &mut Context::from_runtime(&rt);
	let rt = RuntimeBuilder::<(), _>::new()
		.microtask_queue()
		.macrotask_queue()
		.standard_modules(Modules)
		.build(cx);

	if let Some((script, _)) = read_script(path) {
		let (script, sourcemap) = cache(path, script);
		if let Some(sourcemap) = sourcemap {
			save_sourcemap(path, sourcemap);
		}
		let result = Script::compile_and_evaluate(rt.cx(), path, &script);

		match result {
			Ok(v) => println!("{}", format_value(rt.cx(), FormatConfig::default().quoted(true), &v)),
			Err(mut report) => {
				transform_error_report_with_sourcemaps(&mut report);
				eprintln!("{}", report.format(rt.cx()));
			}
		}
		run_event_loop(&rt).await;
	}
}

pub(crate) async fn eval_module(path: &Path) {
	let engine = JSEngine::init().unwrap();
	let rt = RustRuntime::new(engine.handle());

	let cx = &mut Context::from_runtime(&rt);
	let rt = RuntimeBuilder::new()
		.microtask_queue()
		.macrotask_queue()
		.modules(Loader::default())
		.standard_modules(Modules)
		.build(cx);

	if let Some((script, filename)) = read_script(path) {
		let (script, sourcemap) = cache(path, script);
		if let Some(sourcemap) = sourcemap {
			save_sourcemap(path, sourcemap);
		}
		let result = Module::compile_and_evaluate(rt.cx(), &filename, Some(path), &script);

		if let Err(mut error) = result {
			transform_error_report_with_sourcemaps(&mut error.report);
			eprintln!("{}", error.format(rt.cx()));
		}
		run_event_loop(&rt).await;
	}
}

fn read_script(path: &Path) -> Option<(String, String)> {
	match read_to_string(path) {
		Ok(script) => {
			let filename = String::from(path.file_name().unwrap().to_str().unwrap());
			Some((script, filename))
		}
		Err(error) => {
			eprintln!("Failed to read file: {}", path.display());
			match error.kind() {
				ErrorKind::NotFound => eprintln!("(File was not found)"),
				ErrorKind::PermissionDenied => eprintln!("Current User lacks permissions to read the file)"),
				_ => eprintln!("{error:?}"),
			}
			None
		}
	}
}

async fn run_event_loop(rt: &Runtime<'_>) {
	if let Err(err) = rt.run_event_loop().await {
		if let Some(err) = err {
			eprintln!("{}", err.format(rt.cx()));
		} else {
			eprintln!("Unknown error occurred while executing microtask.");
		}
	}
}

fn cache(path: &Path, script: String) -> (String, Option<SourceMap>) {
	let is_typescript = Config::global().typescript && path.extension() == Some(OsStr::new("ts"));
	is_typescript
		.then(|| locate_in_cache(path, &script))
		.flatten()
		.map(|(s, sm)| (s, Some(sm)))
		.unwrap_or_else(|| (script, None))
}
