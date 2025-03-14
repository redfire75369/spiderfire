/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::cmp::Ordering;
use std::fmt;
use std::fmt::{Display, Formatter, Write};

use colored::{Color, Colorize};
use itoa::Buffer;
use mozjs::jsapi::{ESClass, JSProtoKey, Type};
use mozjs::typedarray::{ClampedU8, Float32, Float64, Int8, Int16, Int32, Uint8, Uint16, Uint32};

use crate::conversions::ToValue;
use crate::format::array::format_array;
use crate::format::boxed::format_boxed_primitive;
use crate::format::date::format_date;
use crate::format::descriptor::format_descriptor;
use crate::format::function::format_function;
use crate::format::key::format_key;
use crate::format::promise::format_promise;
use crate::format::regexp::format_regexp;
use crate::format::string::format_string;
use crate::format::typedarray::{format_array_buffer, format_typed_array};
use crate::format::{Config, NEWLINE, indent_str, prefix};
use crate::typedarray::{ArrayBuffer, ArrayBufferView, TypedArray, TypedArrayElement};
use crate::{
	Array, Context, Date, Exception, Function, Local, Object, Promise, PropertyDescriptor, PropertyKey, RegExp,
};

/// Formats a [JavaScript Object](Object), depending on its class, using the given [configuration](Config).
/// The object is passed to more specific formatting functions, such as [format_array] and [format_date].
pub fn format_object<'cx>(cx: &'cx Context, cfg: Config, object: Object<'cx>) -> ObjectDisplay<'cx> {
	ObjectDisplay { cx, object, cfg }
}

#[must_use]
pub struct ObjectDisplay<'cx> {
	cx: &'cx Context,
	object: Object<'cx>,
	cfg: Config,
}

impl Display for ObjectDisplay<'_> {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		use ESClass as ESC;

		let cx = self.cx;
		let cfg = self.cfg;
		let object = Object::from(Local::from_handle(self.object.handle()));

		let class = self.object.get_builtin_class(cx);

		match class {
			ESC::Boolean | ESC::Number | ESC::String | ESC::BigInt => {
				format_boxed_primitive(cx, cfg, &self.object).fmt(f)
			}
			ESC::Array => format_array(cx, cfg, &Array::from(cx, object.into_local()).unwrap()).fmt(f),
			ESC::Date => format_date(cx, cfg, &Date::from(cx, object.into_local()).unwrap()).fmt(f),
			ESC::Promise => format_promise(cx, cfg, &Promise::from(object.into_local()).unwrap()).fmt(f),
			ESC::RegExp => format_regexp(cx, cfg, &RegExp::from(cx, object.into_local()).unwrap()).fmt(f),
			ESC::Function => format_function(cx, cfg, &Function::from_object(cx, &self.object).unwrap()).fmt(f),
			ESC::ArrayBuffer => format_array_buffer(cfg, &ArrayBuffer::from(object.into_local()).unwrap()).fmt(f),
			ESC::Error => match Exception::from_object(cx, &self.object)? {
				Exception::Error(error) => error.format().fmt(f),
				_ => unreachable!("Expected Error"),
			},
			ESC::Object => format_raw_object(cx, cfg, &self.object).fmt(f),
			ESC::Other => {
				if let Some(view) = ArrayBufferView::from(cx.root(object.handle().get())) {
					if let Some(res) = format_array_buffer_view(f, view, cfg) {
						return res;
					}
				}

				format_raw_object(cx, cfg, &self.object).fmt(f)
			}
			_ => format_string(cx, cfg, &self.object.as_value(cx).to_source(cx)).fmt(f),
		}
	}
}

fn format_array_buffer_view(f: &mut Formatter<'_>, view: ArrayBufferView, cfg: Config) -> Option<fmt::Result> {
	fn view_into_array<T: TypedArrayElement>(view: ArrayBufferView) -> Option<TypedArray<T>> {
		TypedArray::from(view.into_local())
	}

	let res = match view.view_type() {
		Type::Int8 => format_typed_array(cfg, &view_into_array::<Int8>(view)?).fmt(f),
		Type::Uint8 => format_typed_array(cfg, &view_into_array::<Uint8>(view)?).fmt(f),
		Type::Int16 => format_typed_array(cfg, &view_into_array::<Int16>(view)?).fmt(f),
		Type::Uint16 => format_typed_array(cfg, &view_into_array::<Uint16>(view)?).fmt(f),
		Type::Int32 => format_typed_array(cfg, &view_into_array::<Int32>(view)?).fmt(f),
		Type::Uint32 => format_typed_array(cfg, &view_into_array::<Uint32>(view)?).fmt(f),
		Type::Float32 => format_typed_array(cfg, &view_into_array::<Float32>(view)?).fmt(f),
		Type::Float64 => format_typed_array(cfg, &view_into_array::<Float64>(view)?).fmt(f),
		Type::Uint8Clamped => format_typed_array(cfg, &view_into_array::<ClampedU8>(view)?).fmt(f),
		_ => return None,
	};
	Some(res)
}

/// Formats an [object](Object) using the given [configuration](Config).
pub fn format_raw_object<'cx>(cx: &'cx Context, cfg: Config, object: &'cx Object<'cx>) -> RawObjectDisplay<'cx> {
	RawObjectDisplay { cx, object, cfg }
}

#[must_use]
pub struct RawObjectDisplay<'cx> {
	cx: &'cx Context,
	object: &'cx Object<'cx>,
	cfg: Config,
}

impl Display for RawObjectDisplay<'_> {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		let colour = self.cfg.colours.object;

		prefix::write_prefix(f, self.cx, self.cfg, self.object, "Object", JSProtoKey::JSProto_Object)?;

		if self.cfg.depth > 3 {
			return "[Object]".color(colour).fmt(f);
		}

		let keys = self.object.keys(self.cx, Some(self.cfg.iteration));
		let length = keys.len();
		if length == 0 {
			return "{}".color(colour).fmt(f);
		}

		"{".color(colour).fmt(f)?;
		if self.cfg.multiline {
			f.write_str(NEWLINE)?;
			let inner = indent_str((self.cfg.indentation + self.cfg.depth + 1) as usize);

			for key in keys {
				inner.fmt(f)?;
				let desc = self.object.get_descriptor(self.cx, &key)?.unwrap();
				write_key_descriptor(f, self.cx, self.cfg, &key, &desc, Some(self.object))?;
				",".color(colour).fmt(f)?;
				f.write_str(NEWLINE)?;
			}

			indent_str((self.cfg.indentation + self.cfg.depth) as usize).fmt(f)?;
		} else {
			f.write_char(' ')?;
			let len = length.clamp(0, 3);

			for (i, key) in keys.enumerate() {
				let desc = self.object.get_descriptor(self.cx, &key)?.unwrap();
				write_key_descriptor(f, self.cx, self.cfg, &key, &desc, Some(self.object))?;

				if i != len - 1 {
					",".color(colour).fmt(f)?;
					f.write_char(' ')?;
				}
			}

			let remaining = length - len;
			write_remaining(f, remaining, None, colour)?;
		}

		"}".color(colour).fmt(f)
	}
}

fn write_key_descriptor(
	f: &mut Formatter, cx: &Context, cfg: Config, key: &PropertyKey, desc: &PropertyDescriptor, object: Option<&Object>,
) -> fmt::Result {
	format_key(cx, cfg, &key.to_owned_key(cx)?).fmt(f)?;
	": ".color(cfg.colours.object).fmt(f)?;
	format_descriptor(cx, cfg, desc, object).fmt(f)
}

pub(crate) fn write_remaining(f: &mut Formatter, remaining: usize, inner: Option<&str>, colour: Color) -> fmt::Result {
	if remaining > 0 {
		if let Some(inner) = inner {
			f.write_str(inner)?;
		}

		match remaining.cmp(&1) {
			Ordering::Equal => "... 1 more item".color(colour).fmt(f)?,
			Ordering::Greater => {
				let mut buffer = Buffer::new();
				write!(
					f,
					"{} {} {}",
					"...".color(colour),
					buffer.format(remaining).color(colour),
					"more items".color(colour)
				)?
			}
			_ => (),
		}
		if inner.is_some() {
			",".color(colour).fmt(f)?;
		} else {
			f.write_char(' ')?;
		}
	}
	Ok(())
}
