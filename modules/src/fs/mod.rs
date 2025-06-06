/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::io;
use std::time::SystemTime;

use chrono::DateTime;
pub use fs::*;
pub use handle::*;
use ion::conversions::ToValue;
use ion::{Context, Date, Error, Object, Value};

mod dir;
mod fs;
mod handle;

pub(crate) fn base_error(base: &str, path: &str, err: &io::Error) -> Error {
	Error::new(format!("Could not {base} {path}: {err}"), None)
}

pub(crate) fn file_error(action: &str, path: &str, err: &io::Error, _: ()) -> Error {
	Error::new(format!("Could not {action} file {path}: {err}"), None)
}

pub(crate) fn seek_error(_: &str, path: &str, err: &io::Error, (mode, offset): (SeekMode, i64)) -> Error {
	Error::new(
		format!("Could not seek file {path} to mode '{mode}' with {offset}: {err}"),
		None,
	)
}

pub(crate) fn dir_error(action: &str, path: &str, err: &io::Error) -> Error {
	Error::new(format!("Could not {action} directory {path}: {err}"), None)
}

pub(crate) fn metadata_error(path: &str, err: &io::Error) -> Error {
	Error::new(format!("Could not get metadata for {path}: {err}"), None)
}

pub(crate) fn translate_error(action: &str, from: &str, to: &str, err: &io::Error) -> Error {
	Error::new(format!("Could not {action} {from} to {to}: {err}"), None)
}

#[derive(Debug)]
pub struct Metadata(pub(crate) std::fs::Metadata);

impl ToValue<'_> for Metadata {
	fn to_value(&self, cx: &Context, value: &mut Value) {
		fn system_time_into_date(cx: &Context, time: io::Result<SystemTime>) -> Option<Date> {
			time.ok().map(|time| Date::from_date(cx, DateTime::from(time)))
		}

		let obj = Object::new(cx);
		obj.set_as(cx, "size", &self.0.len());

		obj.set_as(cx, "isFile", &self.0.is_file());
		obj.set_as(cx, "isDirectory", &self.0.is_dir());
		obj.set_as(cx, "isSymlink", &self.0.is_symlink());

		obj.set_as(cx, "created", &system_time_into_date(cx, self.0.created()));
		obj.set_as(cx, "accessed", &system_time_into_date(cx, self.0.accessed()));
		obj.set_as(cx, "modified", &system_time_into_date(cx, self.0.modified()));

		obj.set_as(cx, "readonly", &self.0.permissions().readonly());

		obj.to_value(cx, value);
	}
}
