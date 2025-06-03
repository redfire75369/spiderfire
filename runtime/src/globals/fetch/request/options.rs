/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::fmt;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use ion::conversions::FromValue;
use ion::{Context, Error, ErrorKind, FromValue, Result, Traceable, Value};
use mozjs::jsapi::JSObject;
use mozjs::jsval::JSVal;
use url::Url;

use crate::globals::fetch::body::FetchBody;
use crate::globals::fetch::header::HeadersInit;

#[derive(Clone, Default, Debug, Traceable)]
pub enum Referrer {
	#[expect(clippy::enum_variant_names)]
	NoReferrer,
	#[default]
	Client,
	Url(#[trace(no_trace)] Url),
}

impl FromStr for Referrer {
	type Err = Error;

	fn from_str(referrer: &str) -> Result<Referrer> {
		if referrer.is_empty() {
			Ok(Referrer::NoReferrer)
		} else {
			let url = Url::parse(referrer).map_err(|e| Error::new(e.to_string(), ErrorKind::Type))?;

			if url.scheme() == "about" && url.path() == "client" {
				Ok(Referrer::Client)
			} else {
				Ok(Referrer::Url(url))
			}
		}
	}
}

impl Display for Referrer {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		match self {
			Referrer::NoReferrer => f.write_str("no-referrer"),
			Referrer::Client => f.write_str("about:client"),
			Referrer::Url(url) => Display::fmt(url, f),
		}
	}
}

impl<'cx> FromValue<'cx> for Referrer {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, strict: bool, _: ()) -> Result<Referrer> {
		let referrer = String::from_value(cx, value, strict, ())?;
		Referrer::from_str(&referrer)
	}
}

#[derive(Copy, Clone, Default, Debug, Traceable)]
pub enum ReferrerPolicy {
	#[default]
	None,
	NoReferrer,
	NoReferrerWhenDowngrade,
	Origin,
	OriginWhenCrossOrigin,
	SameOrigin,
	StrictOrigin,
	StrictOriginWhenCrossOrigin,
	UnsafeUrl,
}

impl FromStr for ReferrerPolicy {
	type Err = Error;

	fn from_str(policy: &str) -> Result<ReferrerPolicy> {
		use ReferrerPolicy as RP;
		match policy {
			"" => Ok(RP::None),
			"no-referrer" => Ok(RP::NoReferrer),
			"no-referrer-when-downgrade" => Ok(RP::NoReferrerWhenDowngrade),
			"origin" => Ok(RP::Origin),
			"origin-when-cross-origin" => Ok(RP::OriginWhenCrossOrigin),
			"same-origin" => Ok(RP::SameOrigin),
			"strict-origin" => Ok(RP::StrictOrigin),
			"strict-origin-when-cross-origin" => Ok(RP::StrictOriginWhenCrossOrigin),
			"unsafe-url" => Ok(RP::UnsafeUrl),
			_ => Err(Error::new(
				"Invalid value for Enumeration ReferrerPolicy",
				ErrorKind::Type,
			)),
		}
	}
}

impl Display for ReferrerPolicy {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		let str = match self {
			ReferrerPolicy::None => "",
			ReferrerPolicy::NoReferrer => "no-referrer",
			ReferrerPolicy::NoReferrerWhenDowngrade => "no-referrer-when-downgrade",
			ReferrerPolicy::Origin => "origin",
			ReferrerPolicy::OriginWhenCrossOrigin => "origin-when-cross-origin",
			ReferrerPolicy::SameOrigin => "same-origin",
			ReferrerPolicy::StrictOrigin => "strict-origin",
			ReferrerPolicy::StrictOriginWhenCrossOrigin => "strict-origin-when-cross-origin",
			ReferrerPolicy::UnsafeUrl => "unsafe-url",
		};
		f.write_str(str)
	}
}

impl<'cx> FromValue<'cx> for ReferrerPolicy {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, _: bool, _: ()) -> Result<ReferrerPolicy> {
		let policy = String::from_value(cx, value, true, ())?;
		ReferrerPolicy::from_str(&policy)
	}
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Traceable)]
pub enum RequestMode {
	SameOrigin,
	Cors,
	#[default]
	NoCors,
	Navigate,
	#[expect(dead_code)]
	Websocket,
}

impl FromStr for RequestMode {
	type Err = Error;

	fn from_str(mode: &str) -> Result<RequestMode> {
		use RequestMode as RM;
		match mode {
			"same-origin" => Ok(RM::SameOrigin),
			"cors" => Ok(RM::Cors),
			"no-cors" => Ok(RM::NoCors),
			"navigate" => Ok(RM::Navigate),
			_ => Err(Error::new("Invalid value for Enumeration RequestMode", ErrorKind::Type)),
		}
	}
}

impl Display for RequestMode {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		let str = match self {
			RequestMode::SameOrigin => "same-origin",
			RequestMode::Cors => "cors",
			RequestMode::NoCors => "no-cors",
			RequestMode::Navigate => "navigate",
			RequestMode::Websocket => "websocket",
		};
		f.write_str(str)
	}
}

impl<'cx> FromValue<'cx> for RequestMode {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, _: bool, _: ()) -> Result<RequestMode> {
		let mode = String::from_value(cx, value, true, ())?;
		RequestMode::from_str(&mode)
	}
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Traceable)]
pub enum RequestCredentials {
	Omit,
	#[default]
	SameOrigin,
	Include,
}

impl FromStr for RequestCredentials {
	type Err = Error;

	fn from_str(credentials: &str) -> Result<RequestCredentials> {
		use RequestCredentials as RC;
		match credentials {
			"omit" => Ok(RC::Omit),
			"same-origin" => Ok(RC::SameOrigin),
			"include" => Ok(RC::Include),
			_ => Err(Error::new(
				"Invalid value for Enumeration RequestCredentials",
				ErrorKind::Type,
			)),
		}
	}
}

impl Display for RequestCredentials {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		let str = match self {
			RequestCredentials::Omit => "omit",
			RequestCredentials::SameOrigin => "same-origin",
			RequestCredentials::Include => "include",
		};
		f.write_str(str)
	}
}

impl<'cx> FromValue<'cx> for RequestCredentials {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, _: bool, _: ()) -> Result<RequestCredentials> {
		let mode = String::from_value(cx, value, true, ())?;
		RequestCredentials::from_str(&mode)
	}
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Traceable)]
pub enum RequestCache {
	#[default]
	Default,
	NoStore,
	Reload,
	NoCache,
	ForceCache,
	OnlyIfCached,
}

impl FromStr for RequestCache {
	type Err = Error;

	fn from_str(credentials: &str) -> Result<RequestCache> {
		use RequestCache as RC;
		match credentials {
			"default" => Ok(RC::Default),
			"no-store" => Ok(RC::NoStore),
			"reload" => Ok(RC::Reload),
			"no-cache" => Ok(RC::NoCache),
			"force-cache" => Ok(RC::ForceCache),
			"only-if-cached" => Ok(RC::OnlyIfCached),
			_ => Err(Error::new(
				"Invalid value for Enumeration RequestCache",
				ErrorKind::Type,
			)),
		}
	}
}

impl Display for RequestCache {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		let str = match self {
			RequestCache::Default => "default",
			RequestCache::NoStore => "no-store",
			RequestCache::Reload => "reload",
			RequestCache::NoCache => "no-cache",
			RequestCache::ForceCache => "force-cache",
			RequestCache::OnlyIfCached => "only-if-cached",
		};
		f.write_str(str)
	}
}

impl<'cx> FromValue<'cx> for RequestCache {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, _: bool, _: ()) -> Result<RequestCache> {
		let mode = String::from_value(cx, value, true, ())?;
		RequestCache::from_str(&mode)
	}
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Traceable)]
pub enum RequestRedirect {
	#[default]
	Follow,
	Error,
	Manual,
}

impl FromStr for RequestRedirect {
	type Err = Error;

	fn from_str(redirect: &str) -> Result<RequestRedirect> {
		use RequestRedirect as RR;
		match redirect {
			"follow" => Ok(RR::Follow),
			"error" => Ok(RR::Error),
			"manual" => Ok(RR::Manual),
			_ => Err(Error::new(
				"Invalid value for Enumeration RequestRedirect",
				ErrorKind::Type,
			)),
		}
	}
}

impl Display for RequestRedirect {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		let str = match self {
			RequestRedirect::Follow => "follow",
			RequestRedirect::Error => "error",
			RequestRedirect::Manual => "manual",
		};
		f.write_str(str)
	}
}

impl<'cx> FromValue<'cx> for RequestRedirect {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, _: bool, _: ()) -> Result<RequestRedirect> {
		let redirect = String::from_value(cx, value, true, ())?;
		RequestRedirect::from_str(&redirect)
	}
}

#[derive(Copy, Clone, Debug, Default, Traceable)]
pub enum RequestDuplex {
	#[default]
	Half,
}

impl FromStr for RequestDuplex {
	type Err = Error;

	fn from_str(redirect: &str) -> Result<RequestDuplex> {
		match redirect {
			"half" => Ok(RequestDuplex::Half),
			_ => Err(Error::new(
				"Invalid value for Enumeration RequestDuplex",
				ErrorKind::Type,
			)),
		}
	}
}

impl<'cx> FromValue<'cx> for RequestDuplex {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, _: bool, _: ()) -> Result<RequestDuplex> {
		let redirect = String::from_value(cx, value, true, ())?;
		RequestDuplex::from_str(&redirect)
	}
}

#[derive(Copy, Clone, Debug, Default, Traceable)]
pub enum RequestPriority {
	High,
	Low,
	#[default]
	Auto,
}

impl FromStr for RequestPriority {
	type Err = Error;

	fn from_str(priority: &str) -> Result<RequestPriority> {
		use RequestPriority as RP;
		match priority {
			"high" => Ok(RP::High),
			"low" => Ok(RP::Low),
			"auto" => Ok(RP::Auto),
			_ => Err(Error::new(
				"Invalid value for Enumeration RequestPriority",
				ErrorKind::Type,
			)),
		}
	}
}

impl<'cx> FromValue<'cx> for RequestPriority {
	type Config = ();

	fn from_value(cx: &'cx Context, value: &Value, _: bool, _: ()) -> Result<RequestPriority> {
		let redirect = String::from_value(cx, value, true, ())?;
		RequestPriority::from_str(&redirect)
	}
}

#[derive(Default, FromValue)]
pub struct RequestInit<'cx> {
	pub(crate) method: Option<String>,
	pub(crate) headers: Option<HeadersInit<'cx>>,
	pub(crate) body: Option<FetchBody>,

	pub(crate) referrer: Option<Referrer>,
	pub(crate) referrer_policy: Option<ReferrerPolicy>,

	pub(crate) mode: Option<RequestMode>,
	pub(crate) credentials: Option<RequestCredentials>,
	pub(crate) cache: Option<RequestCache>,
	pub(crate) redirect: Option<RequestRedirect>,

	pub(crate) integrity: Option<String>,
	pub(crate) keepalive: Option<bool>,
	pub(crate) signal: Option<*mut JSObject>,

	#[expect(dead_code)]
	pub(crate) duplex: Option<RequestDuplex>,
	#[expect(dead_code)]
	#[ion(default)]
	priority: Option<RequestPriority>,
	pub(crate) window: Option<JSVal>,
}
