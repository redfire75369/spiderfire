/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::io;
use std::time::SystemTime;

pub use fs::*;
pub use handle::*;
use ion::{Error, ToValue};

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

#[derive(Debug, ToValue)]
pub struct Metadata {
	size: u64,
	is_file: bool,
	is_directory: bool,
	is_symlink: bool,
	created: Option<SystemTime>,
	accessed: Option<SystemTime>,
	modified: Option<SystemTime>,
	readonly: bool,
}

impl Metadata {
	fn new(meta: &std::fs::Metadata) -> Metadata {
		Metadata {
			size: meta.len(),
			is_file: meta.is_file(),
			is_directory: meta.is_dir(),
			is_symlink: meta.is_symlink(),
			created: meta.created().ok(),
			accessed: meta.accessed().ok(),
			modified: meta.modified().ok(),
			readonly: meta.permissions().readonly(),
		}
	}
}
