/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

mod format;
mod table;

use std::cell::{Cell, RefCell};
use std::collections::hash_map::{Entry, HashMap};
use std::time::Instant;

use ascii_table::{Align, AsciiTable};
use indent::indent_all_by;
use indexmap::IndexSet;
use ion::conversions::FromValue;
use ion::flags::PropertyFlags;
use ion::format::key::format_key;
use ion::format::primitive::format_primitive;
use ion::format::{Config as FormatConfig, format_value, indent_str};
use ion::function::{Opt, Rest};
use ion::{Context, Object, OwnedKey, Result, Stack, Value, function_spec, js_fn};
use mozjs::jsapi::JSFunctionSpec;

use crate::cache::map::find_sourcemap;
use crate::config::{Config, LogLevel};
use crate::globals::console::format::{FormatArg, format_args, format_value_args};
use crate::globals::console::table::{get_cells, sort_keys};

const ANSI_CLEAR: &str = "\x1b[1;1H";
const ANSI_CLEAR_SCREEN_DOWN: &str = "\x1b[0J";

const DEFAULT_LABEL: &str = "default";

thread_local! {
	static COUNT_MAP: RefCell<HashMap<String, u32>> = RefCell::new(HashMap::new());
	static TIMER_MAP: RefCell<HashMap<String, Instant>> = RefCell::new(HashMap::new());

	static INDENTS: Cell<u16> = const { Cell::new(0) };
}

fn log_args(cx: &Context, args: &[Value], log_level: LogLevel) {
	if log_level == LogLevel::None || args.is_empty() {
		return;
	}

	if args.len() == 1 {
		print_args(format_value_args(cx, args.iter()), log_level);
	} else {
		print_args(format_args(cx, args).into_iter(), log_level);
	}
}

fn print_args<'cx>(args: impl Iterator<Item = FormatArg<'cx>>, log_level: LogLevel) {
	if log_level == LogLevel::None {
		return;
	}

	let mut first = true;

	let mut prev_spaced = false;
	for arg in args {
		let spaced = arg.spaced();
		let to_space = !first && (prev_spaced || spaced);
		match log_level {
			LogLevel::Info | LogLevel::Debug => {
				if to_space {
					print!(" ");
				}
				print!("{arg}");
			}
			LogLevel::Warn | LogLevel::Error => {
				if to_space {
					eprint!(" ");
				}
				eprint!("{arg}");
			}
			LogLevel::None => unreachable!(),
		};
		first = false;
		prev_spaced = spaced;
	}
}

fn print_indent(log_level: LogLevel) {
	let indentation = usize::from(INDENTS.get());
	match log_level {
		LogLevel::Info | LogLevel::Debug => print!("{}", indent_str(indentation)),
		LogLevel::Warn | LogLevel::Error => eprint!("{}", indent_str(indentation)),
		LogLevel::None => {}
	}
}

// TODO: Convert to Undefinable<String> as null is a valid label
fn get_label(label: Option<String>) -> String {
	if let Some(label) = label {
		label
	} else {
		String::from(DEFAULT_LABEL)
	}
}

#[js_fn]
fn log(cx: &Context, Rest(values): Rest<Value>) {
	if Config::global().log_level >= LogLevel::Info {
		print_indent(LogLevel::Info);
		log_args(cx, &values, LogLevel::Info);
		println!();
	}
}

#[js_fn]
fn warn(cx: &Context, Rest(values): Rest<Value>) {
	if Config::global().log_level >= LogLevel::Warn {
		print_indent(LogLevel::Warn);
		log_args(cx, &values, LogLevel::Warn);
		println!();
	}
}

#[js_fn]
fn error(cx: &Context, Rest(values): Rest<Value>) {
	if Config::global().log_level >= LogLevel::Error {
		print_indent(LogLevel::Error);
		log_args(cx, &values, LogLevel::Error);
		println!();
	}
}

#[js_fn]
fn debug(cx: &Context, Rest(values): Rest<Value>) {
	if Config::global().log_level == LogLevel::Debug {
		print_indent(LogLevel::Debug);
		log_args(cx, &values, LogLevel::Debug);
		println!();
	}
}

#[js_fn]
fn assert(cx: &Context, Opt(assertion): Opt<bool>, Rest(values): Rest<Value>) {
	if Config::global().log_level >= LogLevel::Error {
		if let Some(assertion) = assertion {
			if assertion {
				return;
			}

			if values.is_empty() {
				print_indent(LogLevel::Error);
				eprintln!("Assertion Failed");
				return;
			}

			if values[0].handle().is_string() {
				print_indent(LogLevel::Error);
				eprint!(
					"Assertion Failed: {} ",
					format_primitive(cx, FormatConfig::default(), &values[0])
				);
				log_args(cx, &values[2..], LogLevel::Error);
				eprintln!();
				return;
			}

			print_indent(LogLevel::Error);
			eprint!("Assertion Failed: ");
			log_args(cx, &values, LogLevel::Error);
			println!();
		} else {
			eprintln!("Assertion Failed:");
		}
	}
}

#[js_fn]
fn clear() {
	INDENTS.set(0);

	println!("{ANSI_CLEAR}");
	println!("{ANSI_CLEAR_SCREEN_DOWN}");
}

#[js_fn]
fn trace(cx: &Context, Rest(values): Rest<Value>) {
	if Config::global().log_level == LogLevel::Debug {
		print_indent(LogLevel::Debug);
		print!("Trace: ");
		log_args(cx, &values, LogLevel::Debug);
		println!();

		let mut stack = Stack::from_capture(cx);
		let indents = ((INDENTS.get() + 1) * 2) as usize;

		if let Some(stack) = &mut stack {
			for record in &mut stack.records {
				if let Some(sourcemap) = find_sourcemap(&record.location.file) {
					record.transform_with_sourcemap(&sourcemap);
				}
			}

			println!("{}", &indent_all_by(indents, stack.format()));
		} else {
			eprintln!("Current Stack could not be captured.");
		}
	}
}

#[js_fn]
fn group(cx: &Context, Rest(values): Rest<Value>) {
	INDENTS.set(INDENTS.get().min(u16::MAX - 1) + 1);

	if Config::global().log_level >= LogLevel::Info {
		log_args(cx, &values, LogLevel::Info);
		println!();
	}
}

#[js_fn]
fn group_end() {
	INDENTS.set(INDENTS.get().max(1) - 1);
}

#[js_fn]
fn count(Opt(label): Opt<String>) {
	let label = get_label(label);
	COUNT_MAP.with_borrow_mut(|counts| {
		let count = match counts.entry(label.clone()) {
			Entry::Vacant(v) => *v.insert(1),
			Entry::Occupied(mut o) => o.insert(o.get() + 1),
		};
		if Config::global().log_level >= LogLevel::Info {
			print_indent(LogLevel::Info);
			println!("{label}: {count}");
		}
	});
}

#[js_fn]
fn count_reset(Opt(label): Opt<String>) {
	let label = get_label(label);
	COUNT_MAP.with_borrow_mut(|counts| match counts.get_mut(&label) {
		Some(count) => {
			*count = 0;
		}
		None => {
			if Config::global().log_level >= LogLevel::Warn {
				print_indent(LogLevel::Warn);
				eprintln!("Count for {label} does not exist");
			}
		}
	});
}

#[js_fn]
fn time(Opt(label): Opt<String>) {
	let label = get_label(label);
	TIMER_MAP.with_borrow_mut(|timers| match timers.entry(label.clone()) {
		Entry::Vacant(v) => {
			v.insert(Instant::now());
		}
		Entry::Occupied(_) => {
			if Config::global().log_level >= LogLevel::Warn {
				print_indent(LogLevel::Warn);
				eprintln!("Timer {label} already exists");
			}
		}
	});
}

#[js_fn]
fn time_log(cx: &Context, Opt(label): Opt<String>, Rest(values): Rest<Value>) {
	let label = get_label(label);
	TIMER_MAP.with_borrow(|timers| match timers.get(&label) {
		Some(start) => {
			if Config::global().log_level >= LogLevel::Info {
				let duration = start.elapsed().as_millis();
				print_indent(LogLevel::Info);
				print!("{label}: {duration}ms ");
				log_args(cx, &values, LogLevel::Info);
				println!();
			}
		}
		None => {
			if Config::global().log_level >= LogLevel::Warn {
				print_indent(LogLevel::Warn);
				eprintln!("Timer {label} does not exist");
			}
		}
	});
}

#[js_fn]
fn time_end(Opt(label): Opt<String>) {
	let label = get_label(label);
	TIMER_MAP.with_borrow_mut(|timers| match timers.remove(&label) {
		Some(start) => {
			if Config::global().log_level >= LogLevel::Info {
				let duration = start.elapsed().as_millis();
				print_indent(LogLevel::Info);
				print!("{label}: {duration}ms - Timer Ended");
				println!();
			}
		}
		None => {
			if Config::global().log_level >= LogLevel::Warn {
				print_indent(LogLevel::Warn);
				eprintln!("Timer {label} does not exist");
			}
		}
	});
}

#[js_fn]
fn table(cx: &Context, data: Value, Opt(columns): Opt<Vec<String>>) -> Result<()> {
	let indents = INDENTS.get();
	if let Ok(object) = Object::from_value(cx, &data, true, ()) {
		let rows = object.keys(cx, None).map(|key| key.to_owned_key(cx));
		let mut has_values = false;

		let (rows, columns) = if let Some(columns) = columns {
			let mut keys = IndexSet::new();
			for column in columns.into_iter() {
				let key = match column.parse::<i32>() {
					Ok(int) => OwnedKey::Int(int),
					Err(_) => OwnedKey::String(column),
				};
				keys.insert(key);
			}

			(sort_keys(cx, rows)?, sort_keys(cx, keys.into_iter().map(Ok))?)
		} else {
			let rows: Vec<_> = rows.collect();
			let mut keys = IndexSet::new();

			for row in &rows {
				let row = row.as_ref().map_err(|e| e.clone())?;
				let value = object.get(cx, row)?.unwrap();
				if let Ok(object) = Object::from_value(cx, &value, true, ()) {
					let obj_keys = object.keys(cx, None).map(|key| key.to_owned_key(cx));
					keys.reserve(obj_keys.len());
					for key in obj_keys {
						keys.insert(key?);
					}
				} else {
					has_values = true;
				}
			}

			(sort_keys(cx, rows)?, sort_keys(cx, keys.into_iter().map(Ok))?)
		};

		let mut table = AsciiTable::default();

		table.column(0).set_header("Indices").set_align(Align::Center);
		for (i, column) in columns.iter().enumerate() {
			let key = format_key(cx, FormatConfig::default(), column);
			table.column(i + 1).set_header(key.to_string()).set_align(Align::Center);
		}
		if has_values {
			table.column(columns.len() + 1).set_header("Values").set_align(Align::Center);
		}

		let cells = get_cells(cx, &object, &rows, &columns, has_values);

		println!("{}", indent_all_by((indents * 2) as usize, table.format(cells)))
	} else if Config::global().log_level >= LogLevel::Info {
		print_indent(LogLevel::Info);
		println!(
			"{}",
			format_value(cx, FormatConfig::default().indentation(indents), &data)
		);
	}

	Ok(())
}

const METHODS: &[JSFunctionSpec] = &[
	function_spec!(log, 0),
	function_spec!(log, c"info", 0),
	function_spec!(log, c"dir", 0),
	function_spec!(log, c"dirxml", 0),
	function_spec!(warn, 0),
	function_spec!(error, 0),
	function_spec!(debug, 0),
	function_spec!(assert, 0),
	function_spec!(clear, 0),
	function_spec!(trace, 0),
	function_spec!(group, 0),
	function_spec!(group, c"groupCollapsed", 0),
	function_spec!(group_end, c"groupEnd", 0),
	function_spec!(count, 1),
	function_spec!(count_reset, c"countReset", 1),
	function_spec!(time, 1),
	function_spec!(time_log, c"timeLog", 1),
	function_spec!(time_end, c"timeEnd", 1),
	function_spec!(table, 1),
	JSFunctionSpec::ZERO,
];

pub fn define(cx: &Context, global: &Object) -> bool {
	let console = Object::new(cx);
	(unsafe { console.define_methods(cx, METHODS) })
		&& global.define_as(cx, "console", &console, PropertyFlags::CONSTANT_ENUMERATED)
}
