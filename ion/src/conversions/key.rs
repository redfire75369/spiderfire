/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::rc::Rc;
use std::string::String as RustString;

use mozjs::jsapi::{JS_StringToId, JS_ValueToId, JSString, PropertyKey as JSPropertyKey, Symbol as JSSymbol};
use mozjs::jsid::{SymbolId, VoidId};
use mozjs::jsval::JSVal;

use crate::symbol::WellKnownSymbolCode;
use crate::{Context, OwnedKey, PropertyKey, String, Symbol, Value};

/// Represents types that can be converted to [property keys](PropertyKey).
pub trait ToPropertyKey<'cx> {
	/// Converts `self` to a new [`PropertyKey`].
	/// Returns `None` when conversion fails.
	fn to_key(&self, cx: &'cx Context) -> Option<PropertyKey<'cx>>;
}

macro_rules! impl_to_key_for_integer {
	($ty:ty) => {
		impl<'cx> ToPropertyKey<'cx> for $ty {
			fn to_key(&self, cx: &'cx Context) -> Option<PropertyKey<'cx>> {
				Some(PropertyKey::with_int(cx, i32::from(*self)))
			}
		}
	};
}

impl_to_key_for_integer!(i8);
impl_to_key_for_integer!(i16);
impl_to_key_for_integer!(i32);

impl_to_key_for_integer!(u8);
impl_to_key_for_integer!(u16);

impl<'cx> ToPropertyKey<'cx> for u32 {
	fn to_key(&self, cx: &'cx Context) -> Option<PropertyKey<'cx>> {
		Some(PropertyKey::with_int(cx, *self as i32))
	}
}

impl<'cx> ToPropertyKey<'cx> for *mut JSString {
	fn to_key(&self, cx: &'cx Context) -> Option<PropertyKey<'cx>> {
		String::from(cx.root(*self)).to_key(cx)
	}
}

impl<'cx> ToPropertyKey<'cx> for String<'cx> {
	fn to_key(&self, cx: &'cx Context) -> Option<PropertyKey<'cx>> {
		let mut key = PropertyKey::from(cx.root(VoidId()));
		(unsafe { JS_StringToId(cx.as_ptr(), self.handle().into(), key.handle_mut().into()) }).then_some(key)
	}
}

impl<'cx> ToPropertyKey<'cx> for RustString {
	fn to_key(&self, cx: &'cx Context) -> Option<PropertyKey<'cx>> {
		String::copy_from_str(cx, self)?.to_key(cx)
	}
}

impl<'cx> ToPropertyKey<'cx> for &str {
	fn to_key(&self, cx: &'cx Context) -> Option<PropertyKey<'cx>> {
		String::copy_from_str(cx, self)?.to_key(cx)
	}
}

impl<'cx> ToPropertyKey<'cx> for *mut JSSymbol {
	fn to_key(&self, cx: &'cx Context) -> Option<PropertyKey<'cx>> {
		Some(cx.root(SymbolId(*self)).into())
	}
}

impl<'cx> ToPropertyKey<'cx> for Symbol<'_> {
	fn to_key(&self, cx: &'cx Context) -> Option<PropertyKey<'cx>> {
		self.handle().to_key(cx)
	}
}

impl<'cx> ToPropertyKey<'cx> for WellKnownSymbolCode {
	fn to_key(&self, cx: &'cx Context) -> Option<PropertyKey<'cx>> {
		Symbol::well_known(cx, *self).to_key(cx)
	}
}

impl<'cx> ToPropertyKey<'cx> for JSVal {
	fn to_key(&self, cx: &'cx Context) -> Option<PropertyKey<'cx>> {
		Value::from(cx.root(*self)).to_key(cx)
	}
}

impl<'cx> ToPropertyKey<'cx> for Value<'cx> {
	fn to_key(&self, cx: &'cx Context) -> Option<PropertyKey<'cx>> {
		let mut key = PropertyKey::from(cx.root(VoidId()));
		(unsafe { JS_ValueToId(cx.as_ptr(), self.handle().into(), key.handle_mut().into()) }).then_some(key)
	}
}

impl<'cx> ToPropertyKey<'cx> for JSPropertyKey {
	fn to_key(&self, cx: &'cx Context) -> Option<PropertyKey<'cx>> {
		Some(cx.root(*self).into())
	}
}

impl<'cx> ToPropertyKey<'cx> for PropertyKey<'cx> {
	fn to_key(&self, cx: &'cx Context) -> Option<PropertyKey<'cx>> {
		self.handle().to_key(cx)
	}
}

impl<'cx> ToPropertyKey<'cx> for OwnedKey<'cx> {
	fn to_key(&self, cx: &'cx Context) -> Option<PropertyKey<'cx>> {
		match self {
			OwnedKey::Int(i) => i.to_key(cx),
			OwnedKey::String(str) => str.to_key(cx),
			OwnedKey::Symbol(symbol) => symbol.to_key(cx),
			OwnedKey::Void => Some(cx.root(VoidId()).into()),
		}
	}
}

impl<'cx, K: ToPropertyKey<'cx>> ToPropertyKey<'cx> for &K {
	fn to_key(&self, cx: &'cx Context) -> Option<PropertyKey<'cx>> {
		(**self).to_key(cx)
	}
}

impl<'cx, K: ToPropertyKey<'cx>> ToPropertyKey<'cx> for Box<K> {
	fn to_key(&self, cx: &'cx Context) -> Option<PropertyKey<'cx>> {
		(**self).to_key(cx)
	}
}

impl<'cx, K: ToPropertyKey<'cx>> ToPropertyKey<'cx> for Rc<K> {
	fn to_key(&self, cx: &'cx Context) -> Option<PropertyKey<'cx>> {
		(**self).to_key(cx)
	}
}

impl<'cx, K: ToPropertyKey<'cx>> ToPropertyKey<'cx> for Option<K> {
	fn to_key(&self, cx: &'cx Context) -> Option<PropertyKey<'cx>> {
		self.as_ref().and_then(|k| k.to_key(cx))
	}
}
