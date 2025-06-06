/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::hash::{Hash, Hasher};
use std::mem::discriminant;
use std::ops::{Deref, DerefMut};

use mozjs::jsapi::{JS_IdToProtoKey, JS_ValueToId, JSProtoKey, PropertyKey as JSPropertyKey, ProtoKeyToId};
use mozjs::jsid::{IntId, VoidId};

use crate::conversions::ToPropertyKey as _;
use crate::{Context, Local, Result, String, Symbol, Value};

pub struct PropertyKey<'k> {
	key: Local<'k, JSPropertyKey>,
}

impl<'k> PropertyKey<'k> {
	/// Creates a [PropertyKey] from an integer.
	pub fn with_int(cx: &'k Context, int: i32) -> PropertyKey<'k> {
		PropertyKey::from(cx.root(IntId(int)))
	}

	/// Creates a [PropertyKey] from a string.
	pub fn with_string(cx: &'k Context, string: &str) -> Option<PropertyKey<'k>> {
		let string = String::copy_from_str(cx, string)?;
		string.to_key(cx)
	}

	pub fn with_symbol(cx: &'k Context, symbol: &Symbol) -> PropertyKey<'k> {
		symbol.to_key(cx).unwrap()
	}

	pub fn from_proto_key(cx: &'k Context, proto_key: JSProtoKey) -> PropertyKey<'k> {
		let mut key = PropertyKey::from(cx.root(VoidId()));
		unsafe { ProtoKeyToId(cx.as_ptr(), proto_key, key.handle_mut().into()) }
		key
	}

	pub fn from_value(cx: &'k Context, value: &Value) -> Option<PropertyKey<'k>> {
		let mut key = PropertyKey::from(cx.root(VoidId()));
		(unsafe { JS_ValueToId(cx.as_ptr(), value.handle().into(), key.handle_mut().into()) }).then_some(key)
	}

	pub fn to_proto_key(&self, cx: &Context) -> Option<JSProtoKey> {
		let proto_key = unsafe { JS_IdToProtoKey(cx.as_ptr(), self.handle().into()) };
		(proto_key != JSProtoKey::JSProto_Null).then_some(proto_key)
	}

	pub fn to_owned_key<'cx>(&self, cx: &'cx Context) -> Result<OwnedKey<'cx>> {
		if self.handle().is_int() {
			Ok(OwnedKey::Int(self.handle().to_int()))
		} else if self.handle().is_string() {
			Ok(OwnedKey::String(
				String::from(cx.root(self.handle().to_string())).to_owned(cx)?,
			))
		} else if self.handle().is_symbol() {
			Ok(OwnedKey::Symbol(cx.root(self.handle().to_symbol()).into()))
		} else {
			Ok(OwnedKey::Void)
		}
	}

	pub fn into_local(self) -> Local<'k, JSPropertyKey> {
		self.key
	}
}

impl<'k> From<Local<'k, JSPropertyKey>> for PropertyKey<'k> {
	fn from(key: Local<'k, JSPropertyKey>) -> PropertyKey<'k> {
		PropertyKey { key }
	}
}

impl<'k> Deref for PropertyKey<'k> {
	type Target = Local<'k, JSPropertyKey>;

	fn deref(&self) -> &Self::Target {
		&self.key
	}
}

impl DerefMut for PropertyKey<'_> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.key
	}
}

/// Represents the key on an object.
#[derive(Debug)]
pub enum OwnedKey<'k> {
	Int(i32),
	String(std::string::String),
	Symbol(Symbol<'k>),
	Void,
}

impl<'k> OwnedKey<'k> {
	pub fn clone(&self, cx: &'k Context) -> OwnedKey<'k> {
		match self {
			OwnedKey::Int(i) => OwnedKey::Int(*i),
			OwnedKey::String(str) => OwnedKey::String(str.clone()),
			OwnedKey::Symbol(symbol) => OwnedKey::Symbol(cx.root(symbol.get()).into()),
			OwnedKey::Void => OwnedKey::Void,
		}
	}
}

impl Hash for OwnedKey<'_> {
	fn hash<H: Hasher>(&self, state: &mut H) {
		discriminant(self).hash(state);
		match self {
			OwnedKey::Int(i) => i.hash(state),
			OwnedKey::String(str) => str.hash(state),
			OwnedKey::Symbol(symbol) => symbol.handle().hash(state),
			OwnedKey::Void => (),
		}
	}
}

impl PartialEq for OwnedKey<'_> {
	fn eq(&self, other: &OwnedKey<'_>) -> bool {
		match (self, other) {
			(OwnedKey::Int(i), OwnedKey::Int(i2)) => *i == *i2,
			(OwnedKey::String(str), OwnedKey::String(str2)) => *str == *str2,
			(OwnedKey::Symbol(symbol), OwnedKey::Symbol(symbol2)) => symbol.get() == symbol2.get(),
			(OwnedKey::Void, OwnedKey::Void) => true,
			_ => false,
		}
	}
}

impl Eq for OwnedKey<'_> {}
