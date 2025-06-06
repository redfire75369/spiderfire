/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::ffi::c_void;
use std::ptr;

use bytes::{Bytes, BytesMut};
use ion::class::Reflector;
use ion::clone::{StructuredCloneBuffer, read_uint64, write_uint64};
use ion::flags::PropertyFlags;
use ion::function::Opt;
use ion::{ClassDefinition as _, Context, FromValue, Local, Object, ResultExc, Value, js_fn};
use mozjs::jsapi::{
	CloneDataPolicy, Handle, JS_ReadBytes, JS_ReadString, JS_WriteBytes, JS_WriteString, JS_WriteUint32Pair, JSContext,
	JSObject, JSStructuredCloneCallbacks, JSStructuredCloneReader, JSStructuredCloneWriter, StructuredCloneScope,
};

use crate::globals::file::Blob;

#[derive(Clone, Copy, Debug)]
#[repr(u32)]
enum StructuredCloneTags {
	Min = 0xFFFF8000,
	BlobSameProcess = 0xFFFF8001,
	BlobDifferentProcess = 0xFFFF8002,
	Max = 0xFFFFFFFF,
}

#[derive(Debug, Default)]
pub struct StructuredCloneDataHolder {
	blob_data: Vec<Bytes>,
}

unsafe extern "C" fn read_callback(
	cx: *mut JSContext, r: *mut JSStructuredCloneReader, _: *const CloneDataPolicy, tag: u32, _data: u32,
	private: *mut c_void,
) -> *mut JSObject {
	assert!(
		tag > StructuredCloneTags::Min as u32,
		"Expected Tag Below StructuredCloneTags::Min"
	);
	assert!(
		tag < StructuredCloneTags::Max as u32,
		"Expected Tag Below StructuredCloneTags::Max"
	);

	let cx = unsafe { &Context::new_unchecked(cx) };
	let data = unsafe { &mut *private.cast::<StructuredCloneDataHolder>() };

	if tag == StructuredCloneTags::BlobSameProcess as u32 {
		let index;
		let mut kind = ion::String::new(cx);

		unsafe {
			index = read_uint64(r).unwrap() as usize;
			JS_ReadString(r, kind.handle_mut().into());
		}

		Blob::new_object(
			cx,
			Box::new(Blob {
				reflector: Reflector::default(),
				bytes: data.blob_data[index].clone(),
				kind: Some(kind.to_owned(cx).unwrap()),
			}),
		)
	} else if tag == StructuredCloneTags::BlobDifferentProcess as u32 {
		let mut bytes;
		let mut kind = ion::String::new(cx);

		unsafe {
			let len = read_uint64(r).unwrap() as usize;
			bytes = BytesMut::with_capacity(len);
			JS_ReadBytes(r, bytes.as_mut_ptr().cast(), len);
			bytes.set_len(len);
			JS_ReadString(r, kind.handle_mut().into());
		}

		Blob::new_object(
			cx,
			Box::new(Blob {
				reflector: Reflector::default(),
				bytes: bytes.freeze(),
				kind: Some(kind.to_owned(cx).unwrap()),
			}),
		)
	} else {
		ptr::null_mut()
	}
}

unsafe extern "C" fn write_callback(
	cx: *mut JSContext, w: *mut JSStructuredCloneWriter, obj: Handle<*mut JSObject>, same_process_scope: *mut bool,
	private: *mut c_void,
) -> bool {
	let cx = unsafe { &Context::new_unchecked(cx) };
	let object = Object::from(unsafe { Local::from_raw_handle(obj) });
	let data = unsafe { &mut *private.cast::<StructuredCloneDataHolder>() };

	if let Ok(blob) = Blob::get_private(cx, &object) {
		let kind = ion::String::copy_from_str(cx, blob.kind.as_deref().unwrap_or("")).unwrap();

		unsafe {
			if *same_process_scope {
				JS_WriteUint32Pair(w, StructuredCloneTags::BlobSameProcess as u32, 0);
				write_uint64(w, data.blob_data.len() as u64);
				data.blob_data.push(blob.bytes.clone());
			} else {
				JS_WriteUint32Pair(w, StructuredCloneTags::BlobDifferentProcess as u32, 0);
				write_uint64(w, blob.bytes.len() as u64);
				JS_WriteBytes(w, blob.bytes.as_ptr().cast(), blob.bytes.len());
			}
			JS_WriteString(w, kind.handle().into());
		}
	}

	true
}

pub static STRUCTURED_CLONE_CALLBACKS: JSStructuredCloneCallbacks = JSStructuredCloneCallbacks {
	read: Some(read_callback),
	write: Some(write_callback),
	reportError: None,
	readTransfer: None,
	writeTransfer: None,
	freeTransfer: None,
	canTransfer: None,
	sabCloned: None,
};

#[derive(FromValue)]
struct StructuredCloneOptions<'cx> {
	transfer: Vec<Object<'cx>>,
}

#[js_fn]
fn structured_clone<'cx>(
	cx: &'cx Context, data: Value<'cx>, Opt(options): Opt<StructuredCloneOptions<'cx>>,
) -> ResultExc<Value<'cx>> {
	let transfer = options.map(|o| o.transfer);
	let policy = CloneDataPolicy {
		allowIntraClusterClonableSharedObjects_: false,
		allowSharedMemoryObjects_: true,
	};

	let mut buffer = StructuredCloneBuffer::new(
		StructuredCloneScope::SameProcess,
		&STRUCTURED_CLONE_CALLBACKS,
		Some(Box::new(StructuredCloneDataHolder::default())),
	);
	buffer.write(cx, &data, transfer, &policy)?;
	buffer.read(cx, &policy)
}

pub fn define(cx: &Context, global: &Object) -> bool {
	!global
		.define_method(
			cx,
			"structuredClone",
			structured_clone,
			1,
			PropertyFlags::CONSTANT_ENUMERATED,
		)
		.handle()
		.is_null()
}
