/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::fmt;
use std::fmt::{Debug, Formatter, Write as _};
use std::marker::PhantomData;

use mozjs::jsapi::JSClass;

use crate::utils::ArrayVec;

pub const MAX_PROTO_CHAIN_LENGTH: usize = 8;

pub trait TypeIdWrap: private::Sealed + 'static {}

mod private {
	use std::any::TypeId;

	pub trait Sealed: 'static {
		fn type_id(&self) -> TypeId;
	}

	impl<T: 'static> Sealed for super::TypeIdWrapper<T> {
		fn type_id(&self) -> TypeId {
			TypeId::of::<T>()
		}
	}
}

pub struct TypeIdWrapper<T: 'static> {
	_private: PhantomData<T>,
}

impl<T: 'static> TypeIdWrapper<T> {
	pub const fn new() -> TypeIdWrapper<T> {
		TypeIdWrapper { _private: PhantomData }
	}
}

impl<T: 'static> Default for TypeIdWrapper<T> {
	fn default() -> TypeIdWrapper<T> {
		TypeIdWrapper::new()
	}
}

impl<T: 'static> TypeIdWrap for TypeIdWrapper<T> {}

unsafe impl<T: 'static> Send for TypeIdWrapper<T> {}

unsafe impl<T: 'static> Sync for TypeIdWrapper<T> {}

pub type PrototypeChain = ArrayVec<MAX_PROTO_CHAIN_LENGTH, &'static (dyn TypeIdWrap + Send + Sync)>;

#[repr(C)]
pub struct NativeClass {
	pub base: JSClass,
	pub prototype_chain: PrototypeChain,
}

impl Debug for NativeClass {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		struct ChainDebug {
			len: usize,
		}

		impl Debug for ChainDebug {
			fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
				f.write_str("[TypeIdWrapper; ")?;
				self.len.fmt(f)?;
				f.write_char('/')?;
				MAX_PROTO_CHAIN_LENGTH.fmt(f)?;
				f.write_char(']')
			}
		}

		f.debug_struct("NativeClass")
			.field("base", &self.base)
			.field("prototype_chain", &ChainDebug { len: self.prototype_chain.len() })
			.finish()
	}
}
