/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::{fs, os};

use ion::class::ClassObjectWrapper;
use ion::flags::PropertyFlags;
use ion::function::Opt;
use ion::{ClassDefinition, Context, Iterator, Object, Promise, Result};
use mozjs::jsapi::{JSFunction, JSFunctionSpec, JSObject};
use runtime::module::NativeModule;
use runtime::promise::future_to_promise;
use tokio::task::spawn_blocking;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_DIRECTORY, FILE_FLAGS_AND_ATTRIBUTES};

use crate::fs::dir::DirIterator;
use crate::fs::{FileHandle, Metadata, base_error, dir_error, file_error, metadata_error, translate_error};

#[derive(Copy, Clone, Debug, FromValue)]
struct OpenOptions {
	#[ion(default = true)]
	read: bool,
	#[ion(default)]
	write: bool,
	#[ion(default)]
	append: bool,
	#[ion(default)]
	truncate: bool,
	#[ion(default)]
	create: bool,
	#[ion(name = "createNew", default)]
	create_new: bool,
}

impl OpenOptions {
	fn into_std(self) -> fs::OpenOptions {
		let mut options = fs::OpenOptions::new();

		options
			.read(self.read)
			.write(self.write)
			.append(self.append)
			.truncate(self.truncate)
			.create(self.create)
			.create_new(self.create_new);

		options
	}

	fn into_tokio(self) -> tokio::fs::OpenOptions {
		let options = self.into_std();
		tokio::fs::OpenOptions::from(options)
	}
}

impl Default for OpenOptions {
	fn default() -> OpenOptions {
		OpenOptions {
			read: true,
			write: false,
			append: false,
			truncate: false,
			create: false,
			create_new: false,
		}
	}
}

#[js_fn]
fn open(cx: &Context, path_str: String, Opt(options): Opt<OpenOptions>) -> Option<Promise> {
	future_to_promise(cx, move |_| async move {
		let path = Path::new(&path_str);
		let options = options.unwrap_or_default().into_tokio();

		match options.open(path).await {
			Ok(file) => Ok(ClassObjectWrapper(Box::new(FileHandle::new(
				&path_str,
				file.into_std().await,
			)))),
			Err(err) => Err(file_error("open", &path_str, err, ())),
		}
	})
}

#[js_fn]
fn open_sync(cx: &Context, path_str: String, Opt(options): Opt<OpenOptions>) -> Result<*mut JSObject> {
	let path = Path::new(&path_str);
	let options = options.unwrap_or_default().into_std();

	match options.open(path) {
		Ok(file) => Ok(FileHandle::new_object(cx, Box::new(FileHandle::new(&path_str, file)))),
		Err(err) => Err(file_error("open", &path_str, err, ())),
	}
}

#[js_fn]
fn create(cx: &Context, path_str: String) -> Option<Promise> {
	future_to_promise(cx, |_| async move {
		let path = Path::new(&path_str);
		let mut options = tokio::fs::OpenOptions::new();
		options.read(true).write(true).truncate(true).create(true);

		match options.open(path).await {
			Ok(file) => Ok(ClassObjectWrapper(Box::new(FileHandle::new(
				&path_str,
				file.into_std().await,
			)))),
			Err(err) => Err(file_error("create", &path_str, err, ())),
		}
	})
}

#[js_fn]
fn create_sync(cx: &Context, path_str: String) -> Result<*mut JSObject> {
	let path = Path::new(&path_str);
	let mut options = fs::OpenOptions::new();
	options.read(true).write(true).truncate(true).create(true);

	match options.open(path) {
		Ok(file) => Ok(FileHandle::new_object(cx, Box::new(FileHandle::new(&path_str, file)))),
		Err(err) => Err(file_error("create", &path_str, err, ())),
	}
}

#[js_fn]
fn metadata(cx: &Context, path_str: String) -> Option<Promise> {
	future_to_promise(cx, |_| async move {
		let path = Path::new(&path_str);
		match tokio::fs::metadata(path).await {
			Ok(meta) => Ok(Metadata(meta)),
			Err(err) => Err(metadata_error(&path_str, err)),
		}
	})
}

#[js_fn]
fn metadata_sync(path_str: String) -> Result<Metadata> {
	let path = Path::new(&path_str);
	match fs::metadata(path) {
		Ok(meta) => Ok(Metadata(meta)),
		Err(err) => Err(metadata_error(&path_str, err)),
	}
}

#[js_fn]
fn link_metadata(cx: &Context, path_str: String) -> Option<Promise> {
	future_to_promise(cx, |_| async move {
		let path = Path::new(&path_str);
		match tokio::fs::symlink_metadata(path).await {
			Ok(meta) => Ok(Metadata(meta)),
			Err(err) => Err(metadata_error(&path_str, err)),
		}
	})
}

#[js_fn]
fn link_metadata_sync(path_str: String) -> Result<Metadata> {
	let path = Path::new(&path_str);
	match fs::symlink_metadata(path) {
		Ok(meta) => Ok(Metadata(meta)),
		Err(err) => Err(metadata_error(&path_str, err)),
	}
}

#[js_fn]
fn read_dir(cx: &Context, path_str: String) -> Option<Promise> {
	future_to_promise(cx, |_| async move {
		let path = PathBuf::from(&path_str);

		spawn_blocking(move || fs::read_dir(path))
			.await
			.unwrap()
			.map(DirIterator::new_iterator)
			.map_err(|err| dir_error("read", &path_str, err))
	})
}

#[js_fn]
fn read_dir_sync(path_str: String) -> Result<Iterator> {
	let path = Path::new(&path_str);

	match fs::read_dir(path) {
		Ok(dir) => Ok(DirIterator::new_iterator(dir)),
		Err(err) => Err(dir_error("read", &path_str, err)),
	}
}

#[js_fn]
fn create_dir(cx: &Context, path_str: String, Opt(recursive): Opt<bool>) -> Option<Promise> {
	future_to_promise(cx, move |_| async move {
		let path = Path::new(&path_str);
		let recursive = recursive.unwrap_or_default();

		let result = if recursive {
			tokio::fs::create_dir_all(path).await
		} else {
			tokio::fs::create_dir(path).await
		};
		match result {
			Ok(_) => Ok(()),
			Err(err) => Err(dir_error("create", &path_str, err)),
		}
	})
}

#[js_fn]
fn create_dir_sync(path_str: String, Opt(recursive): Opt<bool>) -> Result<()> {
	let path = Path::new(&path_str);
	let recursive = recursive.unwrap_or_default();

	let result = if recursive {
		fs::create_dir_all(path)
	} else {
		fs::create_dir(path)
	};
	match result {
		Ok(_) => Ok(()),
		Err(err) => Err(dir_error("create", &path_str, err)),
	}
}

#[js_fn]
fn remove(cx: &Context, path_str: String, Opt(recursive): Opt<bool>) -> Option<Promise> {
	future_to_promise(cx, move |_| async move {
		let path = Path::new(&path_str);
		let recursive = recursive.unwrap_or_default();

		let metadata = tokio::fs::symlink_metadata(path).await?;
		let file_type = metadata.file_type();

		let result = if file_type.is_dir() {
			if recursive {
				tokio::fs::remove_dir_all(path).await
			} else {
				tokio::fs::remove_dir(path).await
			}
		} else {
			#[cfg(unix)]
			{
				tokio::fs::remove_file(path).await
			}

			#[cfg(windows)]
			{
				let attributes = FILE_FLAGS_AND_ATTRIBUTES(metadata.file_attributes());
				if attributes.contains(FILE_ATTRIBUTE_DIRECTORY) {
					tokio::fs::remove_dir(path).await
				} else {
					tokio::fs::remove_file(path).await
				}
			}
		};
		result.map_err(|err| base_error("remove", &path_str, err))
	})
}

#[js_fn]
fn remove_sync(path_str: String, Opt(recursive): Opt<bool>) -> Result<()> {
	let path = Path::new(&path_str);
	let recursive = recursive.unwrap_or_default();

	let metadata = fs::symlink_metadata(path)?;
	let file_type = metadata.file_type();

	let result = if file_type.is_dir() {
		if recursive {
			fs::remove_dir_all(path)
		} else {
			fs::remove_dir(path)
		}
	} else {
		#[cfg(unix)]
		{
			fs::remove_file(path)
		}

		#[cfg(windows)]
		{
			let attributes = FILE_FLAGS_AND_ATTRIBUTES(metadata.file_attributes());
			if attributes.contains(FILE_ATTRIBUTE_DIRECTORY) {
				fs::remove_dir(path)
			} else {
				fs::remove_file(path)
			}
		}
	};
	result.map_err(|err| base_error("remove", &path_str, err))
}

#[js_fn]
fn copy(cx: &Context, from_str: String, to_str: String) -> Option<Promise> {
	future_to_promise(cx, |_| async move {
		let from = Path::new(&from_str);
		let to = Path::new(&to_str);

		tokio::fs::copy(from, to)
			.await
			.map_err(|err| translate_error("copy from", &from_str, &to_str, err))
	})
}

#[js_fn]
fn copy_sync(from_str: String, to_str: String) -> Result<u64> {
	let from = Path::new(&from_str);
	let to = Path::new(&to_str);

	fs::copy(from, to).map_err(|err| translate_error("copy from", &from_str, &to_str, err))
}

#[js_fn]
fn rename(cx: &Context, from_str: String, to_str: String) -> Option<Promise> {
	future_to_promise(cx, |_| async move {
		let from = Path::new(&from_str);
		let to = Path::new(&to_str);

		tokio::fs::rename(from, to)
			.await
			.map_err(|err| translate_error("rename from", &from_str, &to_str, err))
	})
}

#[js_fn]
fn rename_sync(from_str: String, to_str: String) -> Result<()> {
	let from = Path::new(&from_str);
	let to = Path::new(&to_str);

	fs::rename(from, to).map_err(|err| translate_error("rename from", &from_str, &to_str, err))
}

#[js_fn]
fn symlink(cx: &Context, original_str: String, link_str: String) -> Option<Promise> {
	future_to_promise(cx, |_| async move {
		let original = Path::new(&original_str);
		let link = Path::new(&link_str);

		let result;
		#[cfg(target_family = "unix")]
		{
			result = tokio::fs::symlink(original, link).await;
		}
		#[cfg(target_family = "windows")]
		{
			result = if original.is_dir() {
				tokio::fs::symlink_dir(original, link).await
			} else {
				tokio::fs::symlink_file(original, link).await
			};
		}
		result.map_err(|err| translate_error("symlink", &original_str, &link_str, err))
	})
}

#[js_fn]
fn symlink_sync(original_str: String, link_str: String) -> Result<()> {
	let original = Path::new(&original_str);
	let link = Path::new(&link_str);

	let result;
	#[cfg(target_family = "unix")]
	{
		result = os::unix::fs::symlink(original, link);
	}
	#[cfg(target_family = "windows")]
	{
		result = if original.is_dir() {
			os::windows::fs::symlink_dir(original, link)
		} else {
			os::windows::fs::symlink_file(original, link)
		};
	}
	result.map_err(|err| translate_error("symlink", &original_str, &link_str, err))
}

#[js_fn]
fn link(cx: &Context, original_str: String, link_str: String) -> Option<Promise> {
	future_to_promise(cx, |_| async move {
		let original = Path::new(&original_str);
		let link = Path::new(&link_str);

		tokio::fs::hard_link(original, link)
			.await
			.map_err(|err| translate_error("link", &original_str, &link_str, err))
	})
}

#[js_fn]
fn link_sync(original_str: String, link_str: String) -> Result<()> {
	let original = Path::new(&original_str);
	let link = Path::new(&link_str);

	fs::hard_link(original, link).map_err(|err| translate_error("link", &original_str, &link_str, err))
}

#[js_fn]
fn read_link(cx: &Context, path_str: String) -> Option<Promise> {
	future_to_promise(cx, |_| async move {
		let path = Path::new(&path_str);

		match tokio::fs::read_link(&path).await {
			Ok(path) => Ok(path.to_string_lossy().into_owned()),
			Err(err) => Err(base_error("read link", &path_str, err)),
		}
	})
}

#[js_fn]
fn read_link_sync(path_str: String) -> Result<String> {
	let path = Path::new(&path_str);

	match fs::read_link(path) {
		Ok(path) => Ok(path.to_string_lossy().into_owned()),
		Err(err) => Err(base_error("read link", &path_str, err)),
	}
}

#[js_fn]
fn canonical(cx: &Context, path_str: String) -> Option<Promise> {
	future_to_promise(cx, |_| async move {
		let path = Path::new(&path_str);

		match tokio::fs::canonicalize(&path).await {
			Ok(path) => Ok(path.to_string_lossy().into_owned()),
			Err(err) => Err(base_error("read link", &path_str, err)),
		}
	})
}

#[js_fn]
fn canonical_sync(path_str: String) -> Result<String> {
	let path = Path::new(&path_str);

	match fs::canonicalize(path) {
		Ok(path) => Ok(path.to_string_lossy().into_owned()),
		Err(err) => Err(base_error("read link", &path_str, err)),
	}
}

const SYNC_FUNCTIONS: &[JSFunctionSpec] = &[
	function_spec!(open_sync, c"open", 1),
	function_spec!(create_sync, c"create", 1),
	function_spec!(metadata, c"metadata", 1),
	function_spec!(link_metadata, c"linkMetadata", 1),
	function_spec!(read_dir_sync, c"readDir", 1),
	function_spec!(create_dir_sync, c"createDir", 1),
	function_spec!(remove_sync, c"remove", 1),
	function_spec!(copy_sync, c"copy", 2),
	function_spec!(rename_sync, c"rename", 2),
	function_spec!(symlink_sync, c"symlink", 2),
	function_spec!(link_sync, c"link", 2),
	function_spec!(read_link_sync, c"readLink", 1),
	function_spec!(canonical_sync, c"canonical", 1),
	JSFunctionSpec::ZERO,
];

const ASYNC_FUNCTIONS: &[JSFunctionSpec] = &[
	function_spec!(open, 1),
	function_spec!(create, 1),
	function_spec!(metadata, 1),
	function_spec!(link_metadata, c"linkMetadata", 1),
	function_spec!(read_dir, c"readDir", 1),
	function_spec!(create_dir, c"createDir", 1),
	function_spec!(remove, c"remove", 1),
	function_spec!(copy, 2),
	function_spec!(rename, 2),
	function_spec!(symlink, c"symlink", 2),
	function_spec!(link, c"link", 2),
	function_spec!(read_link, c"readLink", 1),
	function_spec!(canonical, c"canonical", 1),
	JSFunctionSpec::ZERO,
];

pub struct FileSystemSync;

impl<'cx> NativeModule<'cx> for FileSystemSync {
	const NAME: &'static str = "fs/sync";
	const VARIABLE_NAME: &'static str = "fsSync";
	const SOURCE: &'static str = include_str!("fs_sync.js");

	fn module(&self, cx: &'cx Context) -> Option<Object<'cx>> {
		let fs = Object::new(cx);

		if unsafe { fs.define_methods(cx, SYNC_FUNCTIONS) } {
			Some(fs)
		} else {
			None
		}
	}
}

pub struct FileSystem<'cx> {
	pub sync: &'cx Object<'cx>,
}

impl<'cx> NativeModule<'cx> for FileSystem<'cx> {
	const NAME: &'static str = "fs";
	const VARIABLE_NAME: &'static str = "fs";
	const SOURCE: &'static str = include_str!("fs.js");

	fn module(&self, cx: &'cx Context) -> Option<Object<'cx>> {
		let fs = Object::new(cx);

		let mut result = unsafe { fs.define_methods(cx, ASYNC_FUNCTIONS) }
			&& fs.define_as(cx, "sync", &self.sync, PropertyFlags::CONSTANT_ENUMERATED)
			&& FileHandle::init_class(cx, &fs).0;

		macro_rules! key {
			($key:literal) => {
				($key, concat!($key, "Sync"))
			};
		}
		const SYNC_KEYS: [(&str, &str); 11] = [
			key!("open"),
			key!("create"),
			key!("readDir"),
			key!("createDir"),
			key!("remove"),
			key!("copy"),
			key!("rename"),
			key!("symlink"),
			key!("link"),
			key!("readLink"),
			key!("canonical"),
		];

		for (key, new_key) in SYNC_KEYS {
			let function = self.sync.get_as::<_, *mut JSFunction>(cx, key, true, ()).ok().flatten();
			result = result
				&& function.is_some()
				&& fs.define_as(cx, new_key, &function.unwrap(), PropertyFlags::CONSTANT_ENUMERATED);
		}

		result.then_some(fs)
	}
}
