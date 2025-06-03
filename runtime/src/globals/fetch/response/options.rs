/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::fmt;
use std::fmt::{Display, Formatter};

use http::StatusCode;
use ion::conversions::FromValue;
use ion::{Context, Error, ErrorKind, FromValue, Result, Traceable, Value};
use mozjs::conversions::ConversionBehavior;

use crate::globals::fetch::header::HeadersInit;

#[derive(Default, FromValue)]
pub struct ResponseInit<'cx> {
	#[ion(default)]
	pub(crate) headers: HeadersInit<'cx>,

	#[ion(default, parser = |s| parse_status(__ion_cx, s))]
	pub(crate) status: StatusCode,
	#[ion(default)]
	pub(crate) status_text: Option<String>,
}

fn parse_status(cx: &Context, status: Value) -> Result<StatusCode> {
	let code = u16::from_value(cx, &status, true, ConversionBehavior::Clamp).map(StatusCode::from_u16);
	match code {
		Ok(Ok(code)) => Ok(code),
		_ => Err(Error::new("Invalid response status code", ErrorKind::Range)),
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Traceable)]
pub enum ResponseKind {
	Basic,
	Cors,
	#[default]
	Default,
	Error,
	Opaque,
	OpaqueRedirect,
}

impl Display for ResponseKind {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		match self {
			ResponseKind::Basic => f.write_str("basic"),
			ResponseKind::Cors => f.write_str("cors"),
			ResponseKind::Default => f.write_str("default"),
			ResponseKind::Error => f.write_str("error"),
			ResponseKind::Opaque => f.write_str("opaque"),
			ResponseKind::OpaqueRedirect => f.write_str("opaqueredirect"),
		}
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ResponseTaint {
	#[default]
	Basic,
	Cors,
	Opaque,
}
