/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::fmt;
use std::fmt::{Display, Formatter, Write};

use colored::{Color, Colorize};
use mozjs::jsapi::{IdentifyStandardPrototype, JS_GetConstructor, JS_GetPrototype, JS_HasInstance, JSProtoKey};

use crate::conversions::ToValue;
use crate::format::Config;
use crate::symbol::WellKnownSymbolCode;
use crate::{Context, Function, Object};

fn get_constructor_name(cx: &Context, object: &Object, proto: &mut Object) -> Option<String> {
	let value = object.as_value(cx);
	let constructor = unsafe {
		JS_GetPrototype(cx.as_ptr(), object.handle().into(), proto.handle_mut().into());
		if proto.handle().get().is_null() {
			return None;
		} else {
			cx.root(JS_GetConstructor(cx.as_ptr(), proto.handle().into()))
		}
	};

	Function::from_object(cx, &constructor)
		.and_then(|constructor_fn| constructor_fn.name(cx).ok())
		.and_then(|name| {
			let mut has_instance = false;
			(unsafe {
				JS_HasInstance(
					cx.as_ptr(),
					constructor.handle().into(),
					value.handle().into(),
					&mut has_instance,
				)
			} && has_instance)
				.then_some(name)
		})
}

fn get_tag(cx: &Context, object: &Object) -> crate::Result<Option<String>> {
	if object.has_own(cx, WellKnownSymbolCode::ToStringTag) {
		if let Some(tag) = object.get_as::<_, String>(cx, WellKnownSymbolCode::ToStringTag, true, ())? {
			return Ok((!tag.is_empty()).then_some(tag));
		}
	}
	Ok(None)
}

fn write_tag(f: &mut Formatter, colour: Color, tag: Option<&str>, fallback: &str) -> fmt::Result {
	if let Some(tag) = tag {
		if tag != fallback {
			"[".color(colour).fmt(f)?;
			tag.color(colour).fmt(f)?;
			"] ".color(colour).fmt(f)?;
		}
	}
	Ok(())
}

pub(crate) fn write_prefix(
	f: &mut Formatter, cx: &Context, cfg: Config, object: &Object, fallback: &str, standard: JSProtoKey,
) -> fmt::Result {
	let mut proto = Object::null(cx);
	let constructor_name = get_constructor_name(cx, object, &mut proto);
	let tag = get_tag(cx, object)?;

	let colour = cfg.colours.object;
	let mut fallback = fallback;
	if let Some(name) = &constructor_name {
		let proto = unsafe { IdentifyStandardPrototype(proto.handle().get()) };
		if proto != standard {
			name.color(colour).fmt(f)?;
			f.write_char(' ')?;
			fallback = name;
		} else if tag.is_some() {
			fallback.color(colour).fmt(f)?;
			f.write_char(' ')?;
		}
	} else {
		"[".color(colour).fmt(f)?;
		fallback.color(colour).fmt(f)?;
		": null prototype] ".color(colour).fmt(f)?;
	}
	write_tag(f, colour, tag.as_deref(), fallback)
}
