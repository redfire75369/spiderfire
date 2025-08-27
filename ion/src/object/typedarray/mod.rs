/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::mem::transmute;
use std::ops::Deref;

pub use buffer::*;
use mozjs::jsapi::{
	Handle, JS_NewDataView, JS_NewFloat32ArrayWithBuffer, JS_NewFloat64ArrayWithBuffer, JS_NewInt8ArrayWithBuffer,
	JS_NewInt16ArrayWithBuffer, JS_NewInt32ArrayWithBuffer, JS_NewUint8ArrayWithBuffer,
	JS_NewUint8ClampedArrayWithBuffer, JS_NewUint16ArrayWithBuffer, JS_NewUint32ArrayWithBuffer, JSContext, JSObject,
	Type,
};
use mozjs::typedarray as jsta;
use mozjs::typedarray::{ArrayBufferU8, ClampedU8, Float32, Float64, Int8, Int16, Int32, Uint8, Uint16, Uint32};
pub use view::*;

use crate::conversions::{IntoValue, ToValue as _};
use crate::{Context, Value};

mod buffer;
mod view;

pub struct ArrayBufferWrapper {
	buf: Box<[<ArrayBufferU8 as jsta::TypedArrayElement>::Element]>,
}

impl ArrayBufferWrapper {
	pub fn into_array_buffer(self, cx: &Context) -> Option<ArrayBuffer<'_>> {
		ArrayBuffer::from_boxed_slice(cx, self.buf)
	}
}

impl<B: Into<Box<[<ArrayBufferU8 as jsta::TypedArrayElement>::Element]>>> From<B> for ArrayBufferWrapper {
	fn from(buffer: B) -> ArrayBufferWrapper {
		ArrayBufferWrapper { buf: buffer.into() }
	}
}

impl Deref for ArrayBufferWrapper {
	type Target = Box<[<ArrayBufferU8 as jsta::TypedArrayElement>::Element]>;

	fn deref(&self) -> &Self::Target {
		&self.buf
	}
}

impl<'cx> IntoValue<'cx> for ArrayBufferWrapper {
	fn into_value(self: Box<Self>, cx: &'cx Context, value: &mut Value) {
		if let Some(buffer) = self.into_array_buffer(cx) {
			buffer.to_value(cx, value);
		}
	}
}

macro_rules! impl_typedarray_wrapper {
	($(($typedarray:ident, $element:ty)$(,)?)*) => {
		$(
			pub struct $typedarray {
				buf: Box<[<$element as jsta::TypedArrayElement>::Element]>,
			}

			impl $typedarray {
				pub fn into_typed_array(self, cx: &Context) -> Option<TypedArray<'_, $element>> {
					TypedArray::from_boxed_slice(cx, self.buf)
				}
			}

			impl<B: Into<Box<[<$element as jsta::TypedArrayElement>::Element]>>> From<B> for $typedarray {
				fn from(buffer: B) -> $typedarray {
					$typedarray { buf: buffer.into() }
				}
			}

			impl Deref for $typedarray {
				type Target = Box<[<$element as jsta::TypedArrayElement>::Element]>;

				fn deref(&self) -> &Self::Target {
					&self.buf
				}
			}

			impl<'cx> IntoValue<'cx> for $typedarray {
				fn into_value(self: Box<Self>, cx: &'cx Context, value: &mut Value) {
					if let Some(array) =  self.into_typed_array(cx) {
						array.to_value(cx, value);
					}
				}
			}
		)*
	};
}

impl_typedarray_wrapper!(
	(Uint8ArrayWrapper, Uint8),
	(Uint16ArrayWrapper, Uint16),
	(Uint32ArrayWrapper, Uint32),
	(Int8ArrayWrapper, Int8),
	(Int16ArrayWrapper, Int16),
	(Int32ArrayWrapper, Int32),
	(Float32ArrayWrapper, Float32),
	(Float64ArrayWrapper, Float64),
	(ClampedUint8ArrayWrapper, ClampedU8),
);

pub type Constructor = unsafe extern "C" fn(*mut JSContext, Handle<*mut JSObject>, usize, i64) -> *mut JSObject;

pub fn type_to_constructor(ty: Type) -> Constructor {
	match ty {
		Type::Int8 => JS_NewInt8ArrayWithBuffer,
		Type::Uint8 => JS_NewUint8ArrayWithBuffer,
		Type::Int16 => JS_NewInt16ArrayWithBuffer,
		Type::Uint16 => JS_NewUint16ArrayWithBuffer,
		Type::Int32 => JS_NewInt32ArrayWithBuffer,
		Type::Uint32 => JS_NewUint32ArrayWithBuffer,
		Type::Float32 => JS_NewFloat32ArrayWithBuffer,
		Type::Float64 => JS_NewFloat64ArrayWithBuffer,
		Type::Uint8Clamped => JS_NewUint8ClampedArrayWithBuffer,
		Type::MaxTypedArrayViewType => unsafe {
			transmute::<
				unsafe extern "C" fn(*mut JSContext, Handle<*mut JSObject>, usize, usize) -> *mut JSObject,
				unsafe extern "C" fn(*mut JSContext, Handle<*mut JSObject>, usize, i64) -> *mut JSObject,
			>(JS_NewDataView)
		},
		_ => unreachable!(),
	}
}

pub fn type_to_element_size(ty: Type) -> usize {
	match ty {
		Type::Int8 | Type::Uint8 | Type::Uint8Clamped | Type::MaxTypedArrayViewType => 1,
		Type::Int16 | Type::Uint16 => 2,
		Type::Int32 | Type::Uint32 | Type::Float32 => 4,
		Type::BigInt64 | Type::BigUint64 | Type::Float64 => 8,
		_ => unreachable!(),
	}
}
