/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::path::{Path, PathBuf};

use ion::utils::normalise_path;
use ion::{Error, ErrorReport, Exception};
use swc_sourcemap::SourceMap;

thread_local!(static SOURCEMAP_CACHE: RefCell<HashMap<PathBuf, SourceMap>> = RefCell::new(HashMap::new()));

pub fn find_sourcemap<P: AsRef<Path>>(path: P) -> Option<SourceMap> {
	SOURCEMAP_CACHE.with_borrow_mut(|cache| {
		let path = path.as_ref().to_path_buf();
		match cache.entry(path) {
			Entry::Occupied(o) => Some(o.get().clone()),
			Entry::Vacant(_) => None,
		}
	})
}

pub fn save_sourcemap<P: AsRef<Path>>(path: P, sourcemap: SourceMap) -> bool {
	SOURCEMAP_CACHE.with_borrow_mut(|cache| {
		let path = normalise_path(path);
		match cache.entry(path) {
			Entry::Vacant(v) => {
				v.insert(sourcemap);
				true
			}
			Entry::Occupied(_) => false,
		}
	})
}

pub fn transform_error_report_with_sourcemaps(report: &mut ErrorReport) {
	if let Exception::Error(Error { location: Some(location), .. }) = &mut report.exception {
		if let Some(sourcemap) = find_sourcemap(&location.file) {
			report.exception.transform_with_sourcemap(&sourcemap);
		}
	}
	if let Some(stack) = &mut report.stack {
		for record in &mut stack.records {
			if let Some(sourcemap) = find_sourcemap(&record.location.file) {
				record.transform_with_sourcemap(&sourcemap);
			}
		}
	}
}
