/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::cell::RefCell;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::future::{Future, poll_fn};
use std::io::{Read, Seek, SeekFrom, Write};
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Arc;
use std::task::Poll;
use std::{io, slice};

use ion::class::Reflector;
use ion::conversions::{ConversionBehavior, FromValue, IntoValue, ToValue};
use ion::function::Opt;
use ion::typedarray::{Uint8Array, Uint8ArrayWrapper};
use ion::{Context, Error, ErrorKind, Promise, Result, TracedHeap, Value};
use mozjs::jsval::DoubleValue;
use runtime::globals::file::BufferSource;
use runtime::promise::future_to_promise;
use tokio::task::spawn_blocking;

use crate::fs::{Metadata, file_error, seek_error};

#[derive(Copy, Clone, Debug, Default)]
pub enum SeekMode {
	#[default]
	Current,
	Start,
	End,
}

impl FromStr for SeekMode {
	type Err = Error;

	fn from_str(redirect: &str) -> Result<SeekMode> {
		use SeekMode as SM;
		match redirect {
			"current" => Ok(SM::Current),
			"start" => Ok(SM::Start),
			"end" => Ok(SM::End),
			_ => Err(Error::new("Invalid value for Enumeration SeekMode", ErrorKind::Type)),
		}
	}
}

impl Display for SeekMode {
	fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
		use SeekMode as SM;
		match self {
			SM::Current => write!(f, "current"),
			SM::Start => write!(f, "start"),
			SM::End => write!(f, "end"),
		}
	}
}

impl<'cx> FromValue<'cx> for SeekMode {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, _: bool, _: ()) -> Result<SeekMode> {
		let redirect = String::from_value(cx, value, true, ())?;
		SeekMode::from_str(&redirect)
	}
}

pub enum ReadResult {
	BytesWritten(usize),
	Buffer(Vec<u8>),
}

impl IntoValue<'_> for ReadResult {
	fn into_value(self: Box<Self>, cx: &Context, value: &mut Value) {
		match *self {
			ReadResult::BytesWritten(bytes) => value.handle_mut().set(DoubleValue(bytes as f64)),
			ReadResult::Buffer(buffer) => Uint8ArrayWrapper::from(buffer).into_typed_array(cx).to_value(cx, value),
		}
	}
}

#[js_class]
pub struct FileHandle {
	reflector: Reflector,
	#[trace(no_trace)]
	path: Arc<str>,
	#[trace(no_trace)]
	handle: Rc<RefCell<Option<File>>>,
}

impl FileHandle {
	pub(crate) fn new(path: &str, file: File) -> FileHandle {
		FileHandle {
			reflector: Reflector::new(),
			path: Arc::from(path),
			handle: Rc::new(RefCell::new(Some(file))),
		}
	}

	pub(crate) fn with_sync<F, T>(&self, callback: F) -> Result<T>
	where
		F: FnOnce(&mut File) -> Result<T>,
	{
		match &mut *self.handle.borrow_mut() {
			Some(handle) => callback(handle),
			None => Err(Error::new("File is busy due to async operation.", None)),
		}
	}

	pub(crate) fn with_blocking_task<F, T>(&self, callback: F) -> impl Future<Output = io::Result<T>> + 'static
	where
		F: FnOnce(&mut File) -> io::Result<T> + Send + 'static,
		T: Send + 'static,
	{
		let handle_cell = Rc::clone(&self.handle);

		async move {
			let mut taken = false;
			let mut handle = poll_fn(|wcx| {
				if let Ok(mut handle) = handle_cell.try_borrow_mut() {
					if let Some(file) = handle.as_mut() {
						let file = file.try_clone().ok().unwrap_or_else(|| {
							taken = true;
							handle.take().unwrap()
						});
						return Poll::Ready(file);
					}
				}

				wcx.waker().wake_by_ref();
				Poll::Pending
			})
			.await;

			let (handle, result) = spawn_blocking(move || {
				let result = callback(&mut handle);
				(handle, result)
			})
			.await
			.unwrap();

			if taken {
				handle_cell.borrow_mut().replace(handle);
			}

			result
		}
	}

	#[expect(clippy::too_many_arguments)]
	pub(crate) fn with_blocking_promise<'cx, F, T, A, E, D>(
		&self, cx: &'cx Context, action: &'static str, path: Arc<str>, callback: F, callback_after: A,
		error_callback: E, error_data: D,
	) -> Option<Promise<'cx>>
	where
		F: FnOnce(&mut File) -> io::Result<T> + Send + 'static,
		T: for<'cx2> IntoValue<'cx2> + Send + 'static,
		A: FnOnce() + 'static,
		E: for<'p> FnOnce(&'static str, &'p str, io::Error, D) -> Error + 'static,
		D: 'static,
	{
		let task = self.with_blocking_task(callback);
		future_to_promise(cx, move |_| async move {
			let result = task.await.map_err(|err| error_callback(action, &path, err, error_data));
			callback_after();
			result
		})
	}
}

#[js_class]
impl FileHandle {
	pub fn read<'cx>(&self, cx: &'cx Context, Opt(array): Opt<Uint8Array>) -> Option<Promise<'cx>> {
		let path = Arc::clone(&self.path);
		let bytes = array.as_ref().map(|array| unsafe {
			let (ptr, len) = array.data();
			slice::from_raw_parts_mut(ptr, len)
		});
		let array = array.as_ref().map(|array| TracedHeap::new(array.handle().get()));

		self.with_blocking_promise(
			cx,
			"read",
			path,
			move |file| read_inner(file, bytes),
			|| drop(array),
			file_error,
			(),
		)
	}

	#[ion(name = "readSync")]
	pub fn read_sync(&self, Opt(array): Opt<Uint8Array>) -> Result<ReadResult> {
		self.with_sync(|file| {
			let bytes = array.as_ref().map(|array| unsafe { array.as_mut_slice() });
			read_inner(file, bytes).map_err(|err| file_error("read", &self.path, err, ()))
		})
	}

	pub fn write<'cx>(
		&self, cx: &'cx Context, #[ion(convert = false)] contents: BufferSource<'cx>,
	) -> Option<Promise<'cx>> {
		let path = Arc::clone(&self.path);
		let contents = contents.to_vec();
		self.with_blocking_promise(
			cx,
			"write",
			path,
			move |file| file.write(&contents).map(|bytes| bytes as u64),
			|| {},
			file_error,
			(),
		)
	}

	#[ion(name = "writeSync")]
	pub fn write_sync(&self, #[ion(convert = false)] contents: BufferSource) -> Result<u64> {
		self.with_sync(|file| {
			let contents = unsafe { contents.as_slice() };
			match file.write(contents) {
				Ok(bytes) => Ok(bytes as u64),
				Err(err) => Err(file_error("write", &self.path, err, ())),
			}
		})
	}

	#[ion(name = "writeAll")]
	pub fn write_all<'cx>(
		&self, cx: &'cx Context, #[ion(convert = false)] contents: BufferSource<'cx>,
	) -> Option<Promise<'cx>> {
		let path = Arc::clone(&self.path);
		let contents = contents.to_vec();
		self.with_blocking_promise(
			cx,
			"write",
			path,
			move |file| file.write_all(&contents).map(|_| ()),
			|| {},
			file_error,
			(),
		)
	}

	#[ion(name = "writeAllSync")]
	pub fn write_all_sync(&self, #[ion(convert = false)] contents: BufferSource) -> Result<()> {
		self.with_sync(|file| {
			let contents = unsafe { contents.as_slice() };
			match file.write_all(contents) {
				Ok(_) => Ok(()),
				Err(err) => Err(file_error("write", &self.path, err, ())),
			}
		})
	}

	pub fn truncate<'cx>(
		&self, cx: &'cx Context, #[ion(convert = ConversionBehavior::EnforceRange
		)]
		Opt(length): Opt<u64>,
	) -> Option<Promise<'cx>> {
		let path = Arc::clone(&self.path);
		self.with_blocking_promise(
			cx,
			"truncate",
			path,
			move |file| file.set_len(length.unwrap_or(0)),
			|| {},
			file_error,
			(),
		)
	}

	#[ion(name = "truncateSync")]
	pub fn truncate_sync(
		&self, #[ion(convert = ConversionBehavior::EnforceRange)] Opt(length): Opt<u64>,
	) -> Result<()> {
		self.with_sync(|file| {
			file.set_len(length.unwrap_or(0))
				.map_err(|err| file_error("truncate", &self.path, err, ()))
		})
	}

	pub fn seek<'cx>(
		&self, cx: &'cx Context, #[ion(convert = ConversionBehavior::EnforceRange)] offset: i64,
		Opt(mode): Opt<SeekMode>,
	) -> Option<Promise<'cx>> {
		let path = Arc::clone(&self.path);
		let mode = mode.unwrap_or_default();
		self.with_blocking_promise(
			cx,
			"seek",
			path,
			move |file| {
				let seek = seek_from_mode(offset, mode);
				file.seek(seek)
			},
			|| {},
			seek_error,
			(mode, offset),
		)
	}

	#[ion(name = "seekSync")]
	pub fn seek_sync(
		&self, #[ion(convert = ConversionBehavior::EnforceRange
		)]
		offset: i64, Opt(mode): Opt<SeekMode>,
	) -> Result<u64> {
		self.with_sync(|file| {
			let mode = mode.unwrap_or_default();
			let seek = seek_from_mode(offset, mode);
			file.seek(seek).map_err(|err| seek_error("", &self.path, err, (mode, offset)))
		})
	}

	pub fn sync<'cx>(&self, cx: &'cx Context) -> Option<Promise<'cx>> {
		let path = Arc::clone(&self.path);
		self.with_blocking_promise(cx, "sync", path, move |file| file.sync_all(), || {}, file_error, ())
	}

	#[ion(name = "syncSync")]
	pub fn sync_sync(&self) -> Result<()> {
		self.with_sync(|file| file.sync_all().map_err(|err| file_error("sync", &self.path, err, ())))
	}

	#[ion(name = "syncData")]
	pub fn sync_data<'cx>(&self, cx: &'cx Context) -> Option<Promise<'cx>> {
		let path = Arc::clone(&self.path);
		self.with_blocking_promise(
			cx,
			"sync data for",
			path,
			move |file| file.sync_data(),
			|| {},
			file_error,
			(),
		)
	}

	#[ion(name = "syncDataSync")]
	pub fn sync_data_sync(&self) -> Result<()> {
		self.with_sync(|file| file.sync_data().map_err(|err| file_error("sync data for", &self.path, err, ())))
	}

	pub fn metadata<'cx>(&self, cx: &'cx Context) -> Option<Promise<'cx>> {
		let path = Arc::clone(&self.path);
		self.with_blocking_promise(
			cx,
			"get metadata for",
			path,
			move |file| file.metadata().map(Metadata),
			|| {},
			file_error,
			(),
		)
	}

	#[ion(name = "metadataSync")]
	pub fn metadata_sync(&self) -> Result<Metadata> {
		self.with_sync(|file| {
			file.metadata()
				.map(Metadata)
				.map_err(|err| file_error("get metadata for", &self.path, err, ()))
		})
	}
}

fn read_inner(file: &mut File, bytes: Option<&mut [u8]>) -> io::Result<ReadResult> {
	if let Some(bytes) = bytes {
		file.read(bytes).map(ReadResult::BytesWritten)
	} else {
		let size = file.metadata().map(|m| m.len() as usize).ok();
		let mut bytes = Vec::new();
		bytes.reserve_exact(size.unwrap_or(0));
		file.read_to_end(&mut bytes).map(|_| ReadResult::Buffer(bytes))
	}
}

fn seek_from_mode(offset: i64, mode: SeekMode) -> SeekFrom {
	use SeekMode as SM;
	match mode {
		SM::Current => SeekFrom::Current(offset),
		SM::Start => SeekFrom::Start(offset.max(0) as u64),
		SM::End => SeekFrom::End(offset),
	}
}
