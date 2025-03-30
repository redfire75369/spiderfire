/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::ffi::c_void;
use std::fmt::{Debug, Formatter};
use std::marker::PhantomData;
use std::mem::size_of;
use std::ops::{Deref, DerefMut};
use std::{fmt, ptr, slice};

use mozjs::jsapi::{
	GetArrayBufferViewLengthAndData, HandleObject, IsArrayBufferViewShared, IsLargeArrayBufferView,
	JS_GetArrayBufferViewBuffer, JS_GetArrayBufferViewByteLength, JS_GetArrayBufferViewByteOffset,
	JS_GetArrayBufferViewType, JS_IsArrayBufferViewObject, JS_NewFloat32ArrayWithBuffer, JS_NewFloat64ArrayWithBuffer,
	JS_NewInt8ArrayWithBuffer, JS_NewInt16ArrayWithBuffer, JS_NewInt32ArrayWithBuffer, JS_NewUint8ArrayWithBuffer,
	JS_NewUint8ClampedArrayWithBuffer, JS_NewUint16ArrayWithBuffer, JS_NewUint32ArrayWithBuffer, JSContext, JSObject,
	NewExternalArrayBuffer, Type,
};
use mozjs::typedarray as jsta;
use mozjs::typedarray::{
	ArrayBufferViewU8, ClampedU8, CreateWith, Float32, Float64, Int8, Int16, Int32, Uint8, Uint16, Uint32,
};

use crate::typedarray::buffer::ArrayBuffer;
use crate::utils::BoxExt;
use crate::{Context, Local, Object};

pub trait TypedArrayElement: jsta::TypedArrayElement {
	const NAME: &'static str;
}

pub trait TypedArrayElementCreator: jsta::TypedArrayElementCreator + TypedArrayElement {
	unsafe fn create_with_buffer(cx: *mut JSContext, object: HandleObject, offset: usize, length: i64)
	-> *mut JSObject;
}

macro_rules! typed_array_elements {
	($(($view:ident, $element:ty $(, $create_with_buffer:ident)?)$(,)?)*) => {
		$(
			pub type $view<'bv> = TypedArray<'bv, $element>;

			impl TypedArrayElement for $element {
				const NAME: &'static str = stringify!($view);
			}

			$(
				impl TypedArrayElementCreator for $element {
					unsafe fn create_with_buffer(
						cx: *mut JSContext, object: HandleObject, offset: usize, length: i64,
					) -> *mut JSObject {
						unsafe {
							$create_with_buffer(cx, object, offset, length)
						}
					}
				}
			)?
		)*
	};

}

typed_array_elements! {
	(ArrayBufferView, ArrayBufferViewU8),
	(Uint8Array, Uint8, JS_NewUint8ArrayWithBuffer),
	(Uint16Array, Uint16, JS_NewUint16ArrayWithBuffer),
	(Uint32Array, Uint32, JS_NewUint32ArrayWithBuffer),
	(Int8Array, Int8, JS_NewInt8ArrayWithBuffer),
	(Int16Array, Int16, JS_NewInt16ArrayWithBuffer),
	(Int32Array, Int32, JS_NewInt32ArrayWithBuffer),
	(Float32Array, Float32, JS_NewFloat32ArrayWithBuffer),
	(Float64Array, Float64, JS_NewFloat64ArrayWithBuffer),
	(ClampedUint8Array, ClampedU8, JS_NewUint8ClampedArrayWithBuffer),
}

pub struct TypedArray<'bv, T: TypedArrayElement> {
	view: Local<'bv, *mut JSObject>,
	_phantom: PhantomData<T>,
}

impl<'bv, T: TypedArrayElementCreator> TypedArray<'bv, T> {
	pub fn create_with(cx: &'bv Context, with: CreateWith<T::Element>) -> Option<TypedArray<'bv, T>> {
		let mut view = Object::null(cx);
		unsafe { jsta::TypedArray::<T, *mut JSObject>::create(cx.as_ptr(), with, view.handle_mut()).ok()? };
		Some(TypedArray {
			view: view.into_local(),
			_phantom: PhantomData,
		})
	}

	/// Creates a new [TypedArray] with the given length.
	pub fn new(cx: &Context, len: usize) -> Option<TypedArray<T>> {
		TypedArray::create_with(cx, CreateWith::Length(len))
	}

	/// Creates a new [TypedArray] by copying the contents of the given slice.
	pub fn copy_from_bytes(cx: &'bv Context, bytes: &[T::Element]) -> Option<TypedArray<'bv, T>> {
		TypedArray::create_with(cx, CreateWith::Slice(bytes))
	}

	/// Creates a new [TypedArray] by transferring ownership of the values to the JS runtime.
	pub fn from_vec(cx: &Context, bytes: Vec<T::Element>) -> Option<TypedArray<T>> {
		TypedArray::from_boxed_slice(cx, bytes.into_boxed_slice())
	}

	/// Creates a new [TypedArray] by transferring ownership of the bytes to the JS runtime.
	pub fn from_boxed_slice(cx: &Context, bytes: Box<[T::Element]>) -> Option<TypedArray<T>> {
		unsafe extern "C" fn free_external_array_buffer<T: TypedArrayElementCreator>(
			contents: *mut c_void, data: *mut c_void,
		) {
			let _ = unsafe { Box::from_raw_parts(contents.cast::<T::Element>(), data as usize) };
		}

		let (ptr, len) = Box::into_raw_parts(bytes);
		let buffer = unsafe {
			NewExternalArrayBuffer(
				cx.as_ptr(),
				len * size_of::<T::Element>(),
				ptr.cast(),
				Some(free_external_array_buffer::<T>),
				len as *mut c_void,
			)
		};

		if buffer.is_null() {
			return None;
		}

		let buffer = ArrayBuffer::from(cx.root(buffer)).unwrap();
		TypedArray::with_array_buffer(cx, &buffer, 0, len)
	}

	/// Creates a new [TypedArray] with a view of the contents of an existing [ArrayBuffer].
	pub fn with_array_buffer(
		cx: &'bv Context, buffer: &ArrayBuffer, byte_offset: usize, len: usize,
	) -> Option<TypedArray<'bv, T>> {
		let view = unsafe { T::create_with_buffer(cx.as_ptr(), buffer.handle().into(), byte_offset, len as i64) };

		if view.is_null() {
			None
		} else {
			Some(TypedArray {
				view: cx.root(view),
				_phantom: PhantomData,
			})
		}
	}
}

impl<'bv, T: TypedArrayElement> TypedArray<'bv, T> {
	pub fn from(object: Local<*mut JSObject>) -> Option<TypedArray<T>> {
		let unwrapped = unsafe { T::unwrap_array(object.get()) };
		if unwrapped.is_null() {
			None
		} else {
			Some(TypedArray { view: object, _phantom: PhantomData })
		}
	}

	pub unsafe fn from_unchecked(object: Local<*mut JSObject>) -> TypedArray<T> {
		TypedArray { view: object, _phantom: PhantomData }
	}

	/// Returns a pointer and length to the contents of the [TypedArray].
	///
	/// The pointer may be invalidated if the underlying [ArrayBuffer] is detached.
	pub fn data(&self) -> (*mut T::Element, usize) {
		let mut len = 0;
		let mut shared = false;
		let mut data = ptr::null_mut();
		unsafe { GetArrayBufferViewLengthAndData(self.get(), &mut len, &mut shared, &mut data) };
		(data.cast::<T::Element>(), len / size_of::<T::Element>())
	}

	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}

	pub fn len(&self) -> usize {
		self.data().1
	}

	/// Returns a slice to the contents of the [TypedArray].
	///
	/// The slice may be invalidated if the underlying [ArrayBuffer] is detached.
	pub unsafe fn as_slice(&self) -> &[T::Element] {
		let (ptr, len) = self.data();
		unsafe { slice::from_raw_parts(ptr, len) }
	}

	/// Returns a mutable slice to the contents of the [TypedArray].
	///
	/// The slice may be invalidated if the underlying [ArrayBuffer] is detached.
	#[expect(clippy::mut_from_ref)]
	pub unsafe fn as_mut_slice(&self) -> &mut [T::Element] {
		let (ptr, len) = self.data();
		unsafe { slice::from_raw_parts_mut(ptr, len) }
	}

	/// Returns the offset of the [TypedArray] with respect to the underlying [ArrayBuffer].
	pub fn offset(&self) -> usize {
		unsafe { JS_GetArrayBufferViewByteOffset(self.get()) }
	}

	/// Returns the length of the [TypedArray] in bytes.
	pub fn byte_length(&self) -> usize {
		unsafe { JS_GetArrayBufferViewByteLength(self.get()) }
	}

	/// Checks if the [TypedArray] is larger than the maximum allowed on 32-bit platforms.
	pub fn is_large(&self) -> bool {
		unsafe { IsLargeArrayBufferView(self.get()) }
	}

	/// Checks if the underlying [ArrayBuffer] is shared.
	pub fn is_shared(&self) -> bool {
		unsafe { IsArrayBufferViewShared(self.get()) }
	}

	/// Returns the underlying [ArrayBuffer]. The buffer may be shared and/or detached.
	pub fn buffer<'ab>(&self, cx: &'ab Context) -> ArrayBuffer<'ab> {
		let mut shared = false;
		ArrayBuffer::from(
			cx.root(unsafe { JS_GetArrayBufferViewBuffer(cx.as_ptr(), self.handle().into(), &mut shared) }),
		)
		.unwrap()
	}

	pub fn into_local(self) -> Local<'bv, *mut JSObject> {
		self.view
	}

	/// Checks if an object is an array buffer view.
	#[expect(clippy::not_unsafe_ptr_arg_deref)]
	pub fn is_array_buffer_view(object: *mut JSObject) -> bool {
		unsafe { JS_IsArrayBufferViewObject(object) }
	}
}

impl TypedArray<'_, ArrayBufferViewU8> {
	pub fn view_type(&self) -> Type {
		unsafe { JS_GetArrayBufferViewType(self.get()) }
	}
}

impl<T: TypedArrayElement> Debug for TypedArray<'_, T> {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		f.debug_struct("TypedArray").field("view", &self.view).finish()
	}
}

impl<'bv, T: TypedArrayElement> Deref for TypedArray<'bv, T> {
	type Target = Local<'bv, *mut JSObject>;

	fn deref(&self) -> &Self::Target {
		&self.view
	}
}

impl<T: TypedArrayElement> DerefMut for TypedArray<'_, T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.view
	}
}
