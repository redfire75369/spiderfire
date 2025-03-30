/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::ffi::c_void;
use std::ops::{Deref, DerefMut};
use std::{ptr, slice};

use mozjs::jsapi::{
	ArrayBufferClone, ArrayBufferCopyData, DetachArrayBuffer, GetArrayBufferMaybeSharedLengthAndData,
	IsArrayBufferObjectMaybeShared, IsDetachedArrayBufferObject, JSObject, NewArrayBufferWithContents,
	NewExternalArrayBuffer, StealArrayBufferContents,
};
use mozjs::typedarray::CreateWith;

use crate::utils::BoxExt;
use crate::{Context, Error, ErrorKind, Local, Object, Result};

#[derive(Debug)]
pub struct ArrayBuffer<'ab> {
	buffer: Local<'ab, *mut JSObject>,
}

impl<'ab> ArrayBuffer<'ab> {
	fn create_with(cx: &'ab Context, with: CreateWith<u8>) -> Option<ArrayBuffer<'ab>> {
		let mut buffer = Object::null(cx);
		unsafe { mozjs::typedarray::ArrayBuffer::create(cx.as_ptr(), with, buffer.handle_mut()).ok()? };
		Some(ArrayBuffer { buffer: buffer.into_local() })
	}

	/// Creates a new [ArrayBuffer] with the given length.
	pub fn new(cx: &Context, len: usize) -> Option<ArrayBuffer> {
		ArrayBuffer::create_with(cx, CreateWith::Length(len))
	}

	/// Creates a new [ArrayBuffer] by copying the contents of the given slice.
	pub fn copy_from_bytes(cx: &'ab Context, bytes: &[u8]) -> Option<ArrayBuffer<'ab>> {
		ArrayBuffer::create_with(cx, CreateWith::Slice(bytes))
	}

	/// Creates a new [ArrayBuffer] by transferring ownership of the bytes to the JS runtime.
	pub fn from_vec(cx: &Context, bytes: Vec<u8>) -> Option<ArrayBuffer> {
		ArrayBuffer::from_boxed_slice(cx, bytes.into_boxed_slice())
	}

	/// Creates a new [ArrayBuffer] by transferring ownership of the bytes to the JS runtime.
	pub fn from_boxed_slice(cx: &Context, bytes: Box<[u8]>) -> Option<ArrayBuffer> {
		unsafe extern "C" fn free_external_array_buffer(contents: *mut c_void, data: *mut c_void) {
			let _ = unsafe { Box::from_raw_parts(contents.cast::<u8>(), data as usize) };
		}

		let (ptr, len) = Box::into_raw_parts(bytes);
		let buffer = unsafe {
			NewExternalArrayBuffer(
				cx.as_ptr(),
				len,
				ptr.cast(),
				Some(free_external_array_buffer),
				len as *mut c_void,
			)
		};

		if buffer.is_null() {
			None
		} else {
			Some(ArrayBuffer { buffer: cx.root(buffer) })
		}
	}

	pub fn from(object: Local<*mut JSObject>) -> Option<ArrayBuffer> {
		if ArrayBuffer::is_array_buffer(object.get()) {
			Some(ArrayBuffer { buffer: object })
		} else {
			None
		}
	}

	pub unsafe fn from_unchecked(object: Local<*mut JSObject>) -> ArrayBuffer {
		ArrayBuffer { buffer: object }
	}

	/// Returns a pointer and length to the contents of the [ArrayBuffer].
	///
	/// The pointer may be invalidated if the [ArrayBuffer] is detached.
	pub fn data(&self) -> (*mut u8, usize, bool) {
		let mut len = 0;
		let mut shared = false;
		let mut data = ptr::null_mut();
		unsafe { GetArrayBufferMaybeSharedLengthAndData(self.get(), &mut len, &mut shared, &mut data) };
		(data, len, shared)
	}

	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}

	pub fn len(&self) -> usize {
		self.data().1
	}

	/// Returns a slice to the contents of the [ArrayBuffer].
	///
	/// The slice may be invalidated if the [ArrayBuffer] is detached.
	pub unsafe fn as_slice(&self) -> &[u8] {
		let (ptr, len, _) = self.data();
		unsafe { slice::from_raw_parts(ptr, len) }
	}

	/// Returns a mutable slice to the contents of the [ArrayBuffer].
	///
	/// The slice may be invalidated if the [ArrayBuffer] is detached.
	#[expect(clippy::mut_from_ref)]
	pub unsafe fn as_mut_slice(&self) -> &mut [u8] {
		let (ptr, len, _) = self.data();
		unsafe { slice::from_raw_parts_mut(ptr, len) }
	}

	/// Clones an [ArrayBuffer].
	pub fn clone<'cx>(&self, cx: &'cx Context, offset: usize, len: usize) -> Option<ArrayBuffer<'cx>> {
		let buffer = unsafe { ArrayBufferClone(cx.as_ptr(), self.handle().into(), offset, len) };
		if buffer.is_null() {
			None
		} else {
			Some(ArrayBuffer { buffer: cx.root(buffer) })
		}
	}

	/// Copies data from one [ArrayBuffer] to another.
	/// Returns `false` if the sizes do not match.
	pub fn copy_data_to(
		&self, cx: &Context, to: &ArrayBuffer, from_index: usize, to_index: usize, count: usize,
	) -> bool {
		unsafe {
			ArrayBufferCopyData(
				cx.as_ptr(),
				to.handle().into(),
				to_index,
				self.handle().into(),
				from_index,
				count,
			)
		}
	}

	pub fn detach(&self, cx: &Context) -> bool {
		unsafe { DetachArrayBuffer(cx.as_ptr(), self.handle().into()) }
	}

	pub fn transfer<'cx>(&self, cx: &'cx Context) -> Result<ArrayBuffer<'cx>> {
		let len = self.len();
		let data = unsafe { StealArrayBufferContents(cx.as_ptr(), self.handle().into()) };
		if data.is_null() {
			return Err(Error::new("ArrayBuffer transfer failed", ErrorKind::Normal));
		}
		let buffer = cx.root(unsafe { NewArrayBufferWithContents(cx.as_ptr(), len, data) });
		if buffer.handle().is_null() {
			return Err(Error::new("ArrayBuffer transfer failed", ErrorKind::Normal));
		}
		Ok(ArrayBuffer { buffer })
	}

	pub fn is_detached(&self) -> bool {
		unsafe { IsDetachedArrayBufferObject(self.get()) }
	}

	pub fn is_shared(&self) -> bool {
		self.data().2
	}

	/// Checks if an object is an array buffer.
	#[expect(clippy::not_unsafe_ptr_arg_deref)]
	pub fn is_array_buffer(object: *mut JSObject) -> bool {
		unsafe { IsArrayBufferObjectMaybeShared(object) }
	}
}

impl<'ab> Deref for ArrayBuffer<'ab> {
	type Target = Local<'ab, *mut JSObject>;

	fn deref(&self) -> &Self::Target {
		&self.buffer
	}
}

impl DerefMut for ArrayBuffer<'_> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.buffer
	}
}
