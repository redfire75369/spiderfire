/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::fs::ReadDir;
use std::iter::Iterator as _;
use std::{fs, result};

use ion::class::Reflector;
use ion::conversions::ToValue as _;
use ion::{ClassDefinition as _, Context, Iterator, JSIterator, Result, Value, js_class};

use crate::fs::{Metadata, metadata_error};

#[js_class]
pub struct DirEntry {
	reflector: Reflector,
	#[trace(no_trace)]
	entry: fs::DirEntry,
}

#[js_class]
impl DirEntry {
	pub fn name(&self) -> String {
		self.entry.file_name().to_string_lossy().into_owned()
	}

	pub fn path(&self) -> String {
		self.entry.path().to_string_lossy().into_owned()
	}

	pub fn metadata(&self) -> Result<Metadata> {
		self.entry
			.metadata()
			.as_ref()
			.map(Metadata::new)
			.map_err(|err| metadata_error(&self.path(), err))
	}
}

pub(crate) struct DirIterator(ReadDir);

impl DirIterator {
	pub(crate) fn new_iterator(dir: ReadDir) -> Iterator {
		Iterator::new(DirIterator(dir), &Value::undefined_handle())
	}
}

impl JSIterator for DirIterator {
	fn next_value<'cx>(&mut self, cx: &'cx Context, _: &Value<'cx>) -> Option<Value<'cx>> {
		let entry = self.0.find(result::Result::is_ok).transpose().unwrap()?;
		let entry = Box::new(DirEntry { reflector: Reflector::new(), entry });
		Some(DirEntry::new_object(cx, entry).as_value(cx))
	}
}
