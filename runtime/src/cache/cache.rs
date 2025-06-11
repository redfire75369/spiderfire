/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::ffi::OsStr;
use std::fmt::{Display, Formatter};
use std::fs::{create_dir_all, metadata, read_dir, read_to_string, remove_dir_all, write};
use std::path::{Path, PathBuf};
use std::str::{Utf8Error, from_utf8};
use std::{fmt, io};

use base64::Engine as _;
use base64::prelude::BASE64_URL_SAFE;
use dirs::home_dir;
use dunce::canonicalize;
use sha3::{Digest as _, Sha3_512};
use swc_sourcemap::SourceMap;

use crate::config::Config;
use crate::typescript;
use crate::typescript::compile_typescript;

pub struct Cache {
	dir: PathBuf,
}

impl Cache {
	pub fn new() -> Option<Cache> {
		home_dir().map(|mut path| {
			path.extend([".spiderfire", "cache"]);
			let _ = create_dir_all(&path);
			Cache { dir: path }
		})
	}

	pub fn dir(&self) -> &Path {
		self.dir.as_path()
	}

	pub fn clear(&self) -> io::Result<()> {
		for entry in read_dir(&self.dir)? {
			remove_dir_all(entry?.path())?;
		}
		create_dir_all(&self.dir)?;
		Ok(())
	}

	pub fn find_folder<P: AsRef<Path>>(&self, path: P) -> Result<PathBuf, Error> {
		let canonical = canonicalize(path)?;
		let folder = canonical.parent().ok_or(Error::Other)?;
		let folder_name = folder.file_name().and_then(OsStr::to_str).ok_or(Error::Other)?;

		let hash = hash(folder.as_os_str().to_str().unwrap().as_bytes(), Some(16));
		let folder = self.dir.join(format!("{folder_name}-{hash}"));
		Ok(folder)
	}

	pub fn check_cache<P: AsRef<Path>>(
		&self, path: P, folder: &Path, source: &str,
	) -> Result<(String, SourceMap), Error> {
		let path = path.as_ref();
		let source_file = path.file_stem().and_then(OsStr::to_str).ok_or(Error::Other)?;
		let extension = path.extension().and_then(OsStr::to_str).ok_or(Error::Other)?;

		let source_hash = hash(source, None);
		let destination_file = folder.join(source_file).with_extension("js");
		let map_file = folder.join(source_file).with_extension("js.map");

		let source_hash_file = folder.join(format!("{source_file}.{extension}.sha512"));
		let destination_hash_file = destination_file.with_extension("js.sha512");
		let map_hash_file = map_file.with_extension("js.map.sha512");

		if folder.exists() && metadata(folder).unwrap().is_dir() && is_file(&source_hash_file) {
			let cached_source_hash = read_to_string(&source_hash_file)?;

			if cached_source_hash.trim() == source_hash
				&& is_file(&destination_file)
				&& is_file(&destination_hash_file)
				&& is_file(&map_file)
				&& is_file(&map_hash_file)
			{
				let destination = read_to_string(&destination_file)?;
				let destination_hash = hash(&destination, None);
				let cached_destination_hash = read_to_string(&destination_hash_file)?;

				let map = read_to_string(&map_file)?;
				let map_hash = hash(&map, None);
				let cached_map_hash = read_to_string(&map_hash_file)?;

				if cached_destination_hash.trim() == destination_hash && cached_map_hash.trim() == map_hash {
					let sourcemap = SourceMap::from_slice(map.as_bytes()).unwrap();
					return Ok((destination, sourcemap));
				}
			}
		}
		Err(Error::HashedSource(source_hash))
	}

	pub fn save_to_cache<P: AsRef<Path>>(
		&self, path: P, folder: &Path, source: &str, source_hash: Option<&str>,
	) -> Result<(String, SourceMap), Error> {
		let path = path.as_ref();
		if Config::global().typescript && path.extension() == Some(OsStr::new("ts")) {
			let source_name = path.file_name().and_then(OsStr::to_str).ok_or(Error::Other)?;
			let source_file = path.file_stem().and_then(OsStr::to_str).ok_or(Error::Other)?;
			let extension = path.extension().and_then(OsStr::to_str).ok_or(Error::Other)?;

			let source_hash = source_hash.map_or_else(|| hash(source, None), String::from);
			let destination_file = folder.join(source_file).with_extension("js");
			let map_file = folder.join(source_file).with_extension("js.map");

			let source_hash_file = folder.join(format!("{source_file}.{extension}.sha512"));
			let destination_hash_file = destination_file.with_extension("js.sha512");
			let map_hash_file = map_file.with_extension("map.sha512");

			let (destination, sourcemap) = compile_typescript(source_name, source)?;
			let mut sourcemap_str: Vec<u8> = Vec::new();
			sourcemap.to_writer(&mut sourcemap_str).unwrap();
			let sourcemap_str = from_utf8(&sourcemap_str)?;

			if !folder.exists() || !metadata(folder)?.is_dir() {
				create_dir_all(folder)?;
			}
			write(&destination_file, &destination)?;
			write(&map_file, sourcemap_str)?;

			write(source_hash_file, source_hash)?;
			write(destination_hash_file, hash(&destination, None))?;
			write(map_hash_file, hash(sourcemap_str, None))?;

			Ok((destination, sourcemap))
		} else {
			Err(Error::Other)
		}
	}
}

#[derive(Debug)]
pub enum Error {
	HashedSource(String),
	Other,
	TypeScript(typescript::Error),
	Io(io::Error),
	FromUtf8(Utf8Error),
}

impl From<io::Error> for Error {
	fn from(err: io::Error) -> Error {
		Error::Io(err)
	}
}

impl From<Utf8Error> for Error {
	fn from(err: Utf8Error) -> Error {
		Error::FromUtf8(err)
	}
}

impl From<typescript::Error> for Error {
	fn from(err: typescript::Error) -> Error {
		Error::TypeScript(err)
	}
}

impl Display for Error {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		match self {
			Error::TypeScript(err) => f.write_str(&err.to_string()),
			Error::Io(err) => f.write_str(&err.to_string()),
			Error::FromUtf8(err) => f.write_str(&err.to_string()),
			_ => Ok(()),
		}
	}
}

fn hash<T: AsRef<[u8]>>(bytes: T, len: Option<usize>) -> String {
	let hash = BASE64_URL_SAFE.encode(Sha3_512::new().chain_update(bytes).finalize());
	len.map_or(hash.clone(), |len| String::from(&hash[0..len]))
}

fn is_file(path: &Path) -> bool {
	path.exists() && metadata(path).unwrap().is_file()
}
