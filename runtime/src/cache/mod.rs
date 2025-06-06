/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::path::Path;

pub use cache::*;
use sourcemap::SourceMap;

mod cache;
pub mod map;

pub fn locate_in_cache<P: AsRef<Path>>(path: P, script: &str) -> Option<(String, SourceMap)> {
	let result = Cache::new().map(|cache| {
		let path = path.as_ref();
		let folder = cache.find_folder(path)?;
		match cache.check_cache(path, &folder, script) {
			Ok(s) => Ok(s),
			Err(Error::HashedSource(hash)) => cache.save_to_cache(path, &folder, script, Some(&hash)),
			Err(Error::Other) => cache.save_to_cache(path, &folder, script, None),
			Err(err) => Err(err),
		}
	});

	match result {
		Some(Ok(s)) => Some(s),
		Some(Err(Error::HashedSource(_) | Error::Other)) | None => None,
		Some(Err(err)) => {
			eprintln!("Error occurred while compiling TypeScript");
			eprintln!("{err}");
			None
		}
	}
}
