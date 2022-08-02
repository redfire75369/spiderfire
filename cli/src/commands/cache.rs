/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::fs::{metadata, read_dir};
use std::io;
use std::path::Path;

use runtime::cache::Cache;

pub fn cache_statistics() {
	if let Some(cache) = Cache::new() {
		println!("Location: {}", cache.dir().display());
		match cache_size(cache.dir()) {
			Ok(size) => println!("Size: {}", format_size(size)),
			Err(err) => eprintln!("Error while Calculating Size: {}", err),
		}
	} else {
		println!("No Cache Found");
	}
}

fn cache_size(folder: &Path) -> io::Result<u64> {
	let mut size = 0;
	let metadata = metadata(folder)?;
	if metadata.is_dir() {
		for entry in read_dir(folder)? {
			size += cache_size(&entry?.path())?;
		}
	} else {
		size += metadata.len();
	}
	Ok(size)
}

const PREFIXES: [&str; 6] = ["", "Ki", "Mi", "Gi", "Ti", "Pi"];

fn format_size(size: u64) -> String {
	if size >= 1024 {
		let index: u32 = f64::log(size as f64, 1024.0).floor() as u32;
		let s1 = size / 1024_u64.pow(index);
		let s2 = (size - s1 * 1024_u64.pow(index)) / 1024_u64.pow(index - 1);

		if s2 != 0 {
			format!("{} {}B, {} {}B", s1, PREFIXES[index as usize], s2, PREFIXES[index as usize - 1])
		} else {
			format!("{} {}B", s1, PREFIXES[index as usize])
		}
	} else {
		format!("{} B", size)
	}
}
