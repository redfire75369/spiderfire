/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::iter::once;
use std::mem::take;
use std::str;
use std::str::FromStr;

use async_recursion::async_recursion;
use bytes::Bytes;
use const_format::concatcp;
use data_url::DataUrl;
use futures::future::{Either, select};
use http::{HeaderMap, HeaderValue, Method, StatusCode};
use http::header::{
	ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, ACCESS_CONTROL_ALLOW_HEADERS, CACHE_CONTROL, CONTENT_ENCODING,
	CONTENT_LANGUAGE, CONTENT_LENGTH, CONTENT_LOCATION, CONTENT_TYPE, HOST, IF_MATCH, IF_MODIFIED_SINCE, IF_NONE_MATCH,
	IF_RANGE, IF_UNMODIFIED_SINCE, LOCATION, PRAGMA, RANGE, REFERER, REFERRER_POLICY, USER_AGENT,
};
use mozjs::jsapi::JSObject;
use sys_locale::get_locales;
use tokio::fs::read;
use url::Url;

pub use client::{default_client, GLOBAL_CLIENT};
pub use header::Headers;
use ion::{ClassDefinition, Context, Error, ErrorKind, Exception, Object, Promise, ResultExc};
use ion::class::Reflector;
use ion::conversions::ToValue;
use ion::flags::PropertyFlags;
pub use request::{Request, RequestInfo, RequestInit};
pub use response::Response;

use crate::globals::abort::AbortSignal;
use crate::globals::fetch::body::FetchBody;
use crate::globals::fetch::client::Client;
use crate::globals::fetch::header::{FORBIDDEN_RESPONSE_HEADERS, HeadersKind, remove_all_header_entries};
use crate::globals::fetch::request::{
	Referrer, ReferrerPolicy, RequestCache, RequestCredentials, RequestMode, RequestRedirect,
};
use crate::globals::fetch::response::{network_error, ResponseKind, ResponseTaint};
use crate::promise::future_to_promise;
use crate::VERSION;

mod body;
mod client;
mod header;
mod request;
mod response;

const DEFAULT_USER_AGENT: &str = concatcp!("Spiderfire/", VERSION);

#[js_fn]
fn fetch(cx: &Context, resource: RequestInfo, init: Option<RequestInit>) -> Option<Promise> {
	let promise = Promise::new(cx);

	let mut request = match Request::constructor(cx, resource, init) {
		Ok(request) => request,
		Err(error) => {
			promise.reject(cx, &error.to_value(cx).unwrap());
			return Some(promise);
		}
	};

	let signal = AbortSignal::get_private(&request.signal);
	if let Some(reason) = signal.get_reason() {
		promise.reject(cx, &cx.root(reason).into());
		return Some(promise);
	}

	let headers = Headers::get_mut_private(request.headers.as_mut().unwrap());
	if !headers.headers.contains_key(ACCEPT) {
		headers.headers.append(ACCEPT, HeaderValue::from_static("*/*"));
	}

	let mut locales = get_locales().enumerate();
	let mut locale_string = locales.next().map(|(_, s)| s).unwrap_or_else(|| String::from("*"));
	for (index, locale) in locales {
		locale_string.push(',');
		locale_string.push_str(&locale);
		locale_string.push_str(";q=0.");
		locale_string.push_str(&(1000 - index).to_string());
	}
	if !headers.headers.contains_key(ACCEPT_LANGUAGE) {
		headers.headers.append(ACCEPT_LANGUAGE, HeaderValue::from_str(&locale_string).unwrap());
	}

	let cx2 = unsafe { Context::new_unchecked(cx.as_ptr()) };
	let mut request = Object::from(cx.root(Request::new_object(cx, Box::new(request))));
	future_to_promise(cx, async move {
		fetch_internal(&cx2, &mut request, GLOBAL_CLIENT.get().unwrap().clone()).await
	})
}

async fn fetch_internal(cx: &Context, request: &mut Object, client: Client) -> ResultExc<*mut JSObject> {
	let request = Request::get_mut_private(request);
	let signal = AbortSignal::get_private(&request.signal).signal.clone().poll();
	let send = Box::pin(main_fetch(cx, request, client, 0));
	let response = match select(send, signal).await {
		Either::Left((response, _)) => Ok(response),
		Either::Right((exception, _)) => Err(Exception::Other(exception)),
	};
	response.and_then(|response| {
		if response.kind == ResponseKind::Error {
			Err(Exception::Error(Error::new(
				&format!("Network Error: Failed to fetch from {}", &request.url),
				ErrorKind::Type,
			)))
		} else {
			Ok(Response::new_object(cx, Box::new(response)))
		}
	})
}

static BAD_PORTS: &[u16] = &[
	1,     // tcpmux
	7,     // echo
	9,     // discard
	11,    // systat
	13,    // daytime
	15,    // netstat
	17,    // qotd
	19,    // chargen
	20,    // ftp-data
	21,    // ftp
	22,    // ssh
	23,    // telnet
	25,    // smtp
	37,    // time
	42,    // name
	43,    // nicname
	53,    // domain
	69,    // tftp
	77,    // —
	79,    // finger
	87,    // —
	95,    // supdup
	101,   // hostname
	102,   // iso-tsap
	103,   // gppitnp
	104,   // acr-nema
	109,   // pop2
	110,   // pop3
	111,   // sunrpc
	113,   // auth
	115,   // sftp
	117,   // uucp-path
	119,   // nntp
	123,   // ntp
	135,   // epmap
	137,   // netbios-ns
	139,   // netbios-ssn
	143,   // imap
	161,   // snmp
	179,   // bgp
	389,   // ldap
	427,   // svrloc
	465,   // submissions
	512,   // exec
	513,   // login
	514,   // shell
	515,   // printer
	526,   // tempo
	530,   // courier
	531,   // chat
	532,   // netnews
	540,   // uucp
	548,   // afp
	554,   // rtsp
	556,   // remotefs
	563,   // nntps
	587,   // submission
	601,   // syslog-conn
	636,   // ldaps
	989,   // ftps-data
	990,   // ftps
	993,   // imaps
	995,   // pop3s
	1719,  // h323gatestat
	1720,  // h323hostcall
	1723,  // pptp
	2049,  // nfs
	3659,  // apple-sasl
	4045,  // npp
	5060,  // sip
	5061,  // sips
	6000,  // x11
	6566,  // sane-port
	6665,  // ircu
	6666,  // ircu
	6667,  // ircu
	6668,  // ircu
	6669,  // ircu
	6697,  // ircs-u
	10080, // amanda
];

static SCHEMES: [&str; 4] = ["about", "blob", "data", "file"];

#[async_recursion(?Send)]
async fn main_fetch(cx: &Context, request: &mut Request, client: Client, redirections: u8) -> Response {
	let scheme = request.url.scheme();

	// TODO: Upgrade HTTP Schemes if the host is a domain and matches the Known HSTS Domain List

	let mut taint = ResponseTaint::default();
	let mut opaque_redirect = false;
	let mut response = {
		if request.mode == RequestMode::SameOrigin {
			network_error(cx)
		} else if SCHEMES.contains(&scheme) {
			scheme_fetch(cx, scheme, request.url.clone()).await
		} else if scheme == "https" || scheme == "http" {
			if let Some(port) = request.url.port() {
				if BAD_PORTS.contains(&port) {
					return network_error(cx);
				}
			}
			if request.mode == RequestMode::NoCors {
				if request.redirect != RequestRedirect::Follow {
					return network_error(cx);
				}
			} else {
				taint = ResponseTaint::Cors;
			}
			let (response, opaque) = http_fetch(cx, request, client, taint, redirections).await;
			opaque_redirect = opaque;
			response
		} else {
			network_error(cx)
		}
	};

	let redirected = redirections > 0;
	if redirected || response.kind == ResponseKind::Error {
		response.redirected = redirected;
		return response;
	}

	response.url.get_or_insert(request.url.clone());

	let headers = Headers::get_mut_private(&mut response.headers);

	if !opaque_redirect
		&& taint == ResponseTaint::Opaque
		&& response.status == Some(StatusCode::PARTIAL_CONTENT)
		&& response.range_requested
		&& !headers.headers.contains_key(RANGE)
	{
		let url = response.url.take().unwrap();
		response = network_error(cx);
		response.url = Some(url);
		return response;
	}

	if !opaque_redirect
		&& (request.request.method() == Method::HEAD
		|| request.request.method() == Method::CONNECT
		|| response.status == Some(StatusCode::SWITCHING_PROTOCOLS)
		|| response.status.as_ref().map(StatusCode::as_u16) == Some(103) // Early Hints
		|| response.status == Some(StatusCode::NO_CONTENT)
		|| response.status == Some(StatusCode::RESET_CONTENT)
		|| response.status == Some(StatusCode::NOT_MODIFIED))
	{
		response.body = None;
	}

	if opaque_redirect {
		response.kind = ResponseKind::OpaqueRedirect;
		response.url = None;
		response.status = None;
		response.status_text = None;
		response.body = None;

		headers.headers.clear();
	} else {
		match taint {
			ResponseTaint::Basic => {
				response.kind = ResponseKind::Basic;

				for name in &FORBIDDEN_RESPONSE_HEADERS {
					remove_all_header_entries(&mut headers.headers, name);
				}
			}
			ResponseTaint::Cors => {
				response.kind = ResponseKind::Cors;

				let mut allows_all = false;
				let allowed: Vec<_> = headers
					.headers
					.get_all(ACCESS_CONTROL_ALLOW_HEADERS)
					.into_iter()
					.map(|v| {
						if v == "*" {
							allows_all = true
						}
						v.clone()
					})
					.collect();
				let mut to_remove = Vec::new();
				if request.credentials != RequestCredentials::Include && allows_all {
					for name in headers.headers.keys() {
						if headers.headers.get_all(name).into_iter().size_hint().1.is_none() {
							to_remove.push(name.clone());
						}
					}
				} else {
					for name in headers.headers.keys() {
						let allowed = allowed.iter().any(|allowed| allowed.as_bytes() == name.as_str().as_bytes());
						if allowed {
							to_remove.push(name.clone());
						}
					}
				}
				for name in to_remove {
					remove_all_header_entries(&mut headers.headers, &name);
				}
				for name in &FORBIDDEN_RESPONSE_HEADERS {
					remove_all_header_entries(&mut headers.headers, name);
				}
			}
			ResponseTaint::Opaque => {
				response.kind = ResponseKind::Opaque;
				response.url = None;
				response.status = None;
				response.status_text = None;
				response.body = None;

				headers.headers.clear();
			}
		}
	}

	response
}

async fn scheme_fetch(cx: &Context, scheme: &str, url: Url) -> Response {
	match scheme {
		"about" if url.path() == "blank" => {
			let headers = Headers {
				reflector: Reflector::default(),
				headers: HeaderMap::from_iter(once((
					CONTENT_TYPE,
					HeaderValue::from_static("text/html;charset=UTF-8"),
				))),
				kind: HeadersKind::Immutable,
			};

			Response::new_from_bytes(cx, Bytes::default(), url, headers)
		}
		// TODO: blob: URLs
		"data" => {
			let data_url = match DataUrl::process(url.as_str()) {
				Ok(data_url) => data_url,
				Err(_) => return network_error(cx),
			};

			let (body, _) = match data_url.decode_to_vec() {
				Ok(decoded) => decoded,
				Err(_) => return network_error(cx),
			};
			let mime = data_url.mime_type();
			let mime = format!("{}/{}", mime.type_, mime.subtype);

			let headers = Headers {
				reflector: Reflector::default(),
				headers: HeaderMap::from_iter(once((CONTENT_TYPE, HeaderValue::from_str(&mime).unwrap()))),
				kind: HeadersKind::Immutable,
			};
			Response::new_from_bytes(cx, Bytes::from(body), url, headers)
		}
		"file" => {
			let path = url.to_file_path().unwrap();
			match read(path).await {
				Ok(bytes) => {
					let headers = Headers::new(HeadersKind::Immutable);
					Response::new_from_bytes(cx, Bytes::from(bytes), url, headers)
				}
				Err(_) => network_error(cx),
			}
		}
		_ => network_error(cx),
	}
}

async fn http_fetch(
	cx: &Context, request: &mut Request, client: Client, taint: ResponseTaint, redirections: u8,
) -> (Response, bool) {
	let response = http_network_fetch(cx, request, client.clone(), false).await;
	match response.status {
		Some(status) if status.is_redirection() => match request.redirect {
			RequestRedirect::Follow => (
				http_redirect_fetch(cx, request, response, client, taint, redirections).await,
				false,
			),
			RequestRedirect::Error => (network_error(cx), false),
			RequestRedirect::Manual => (response, true),
		},
		_ => (response, false),
	}
}

#[async_recursion(?Send)]
async fn http_network_fetch(cx: &Context, req: &mut Request, client: Client, is_new: bool) -> Response {
	let mut request = req.clone();
	let headers = Headers::get_mut_private(req.headers.as_mut().unwrap());
	*request.request.headers_mut() = headers.headers.clone();

	let length = request.body.len().or_else(|| {
		(request.body.is_none()
			&& (request.request.method() == Method::POST || request.request.method() == Method::PUT))
			.then_some(0)
	});

	let headers = request.request.headers_mut();
	if let Some(length) = length {
		headers.append(CONTENT_LENGTH, HeaderValue::from_str(&length.to_string()).unwrap());
	}

	if let Referrer::Url(url) = request.referrer {
		headers.append(REFERER, HeaderValue::from_str(url.as_str()).unwrap());
	}

	if !headers.contains_key(USER_AGENT) {
		headers.append(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
	}

	if request.cache == RequestCache::Default
		&& (headers.contains_key(IF_MODIFIED_SINCE)
			|| headers.contains_key(IF_NONE_MATCH)
			|| headers.contains_key(IF_UNMODIFIED_SINCE)
			|| headers.contains_key(IF_MATCH)
			|| headers.contains_key(IF_RANGE))
	{
		request.cache = RequestCache::NoStore;
	}

	if request.cache == RequestCache::NoCache && !headers.contains_key(CACHE_CONTROL) {
		headers.append(CACHE_CONTROL, HeaderValue::from_static("max-age=0"));
	}

	if request.cache == RequestCache::NoStore || request.cache == RequestCache::Reload {
		if !headers.contains_key(PRAGMA) {
			headers.append(PRAGMA, HeaderValue::from_static("no-cache"));
		}
		if !headers.contains_key(CACHE_CONTROL) {
			headers.append(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
		}
	}

	if headers.contains_key(RANGE) {
		headers.append(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
	}

	if !headers.contains_key(HOST) {
		let host = request
			.url
			.host_str()
			.map(|host| {
				if let Some(port) = request.url.port() {
					format!("{}:{}", host, port)
				} else {
					String::from(host)
				}
			})
			.unwrap();
		headers.append(HOST, HeaderValue::from_str(&host).unwrap());
	}

	if request.cache == RequestCache::OnlyIfCached {
		return network_error(cx);
	}

	let range_requested = headers.contains_key(RANGE);

	let mut response = match client.request(request.request).await {
		Ok(mut response) => {
			let headers = Headers {
				reflector: Reflector::default(),
				headers: take(response.headers_mut()),
				kind: HeadersKind::Immutable,
			};

			Response::new(cx, response, req.url.clone(), headers)
		}
		Err(_) => return network_error(cx),
	};

	response.range_requested = range_requested;

	if response.status == Some(StatusCode::PROXY_AUTHENTICATION_REQUIRED) && !req.client_window {
		return network_error(cx);
	}

	if response.status == Some(StatusCode::MISDIRECTED_REQUEST) && !is_new && req.body.is_not_stream() {
		return http_network_fetch(cx, req, client, true).await;
	}

	response
}

async fn http_redirect_fetch(
	cx: &Context, request: &mut Request, response: Response, client: Client, taint: ResponseTaint, redirections: u8,
) -> Response {
	let headers = Headers::get_private(&response.headers);
	let mut location = headers.headers.get_all(LOCATION).into_iter();
	let location = match location.size_hint().1 {
		Some(0) => return response,
		None => return network_error(cx),
		_ => {
			let location = location.next().unwrap();
			match Url::options().base_url(response.url.as_ref()).parse(str::from_utf8(location.as_bytes()).unwrap()) {
				Ok(mut url) => {
					if url.fragment().is_none() {
						url.set_fragment(response.url.as_ref().and_then(Url::fragment));
					}
					url
				}
				Err(_) => return network_error(cx),
			}
		}
	};

	if !(location.scheme() == "https" || location.scheme() == "http") {
		return network_error(cx);
	}

	if redirections >= 20 {
		return network_error(cx);
	}

	if taint == ResponseTaint::Cors && (location.username() != "" || location.password().is_some()) {
		return network_error(cx);
	}

	if response.status != Some(StatusCode::SEE_OTHER) && !request.body.is_none() && !request.body.is_not_stream() {
		return network_error(cx);
	}

	if ((response.status == Some(StatusCode::MOVED_PERMANENTLY) || response.status == Some(StatusCode::FOUND))
		&& request.request.method() == Method::POST)
		|| (response.status == Some(StatusCode::SEE_OTHER)
			&& (request.request.method() != Method::GET || request.request.method() != Method::HEAD))
	{
		*request.request.method_mut() = Method::GET;
		request.body = FetchBody::default();
		let headers = Headers::get_mut_private(request.headers.as_mut().unwrap());
		remove_all_header_entries(&mut headers.headers, &CONTENT_ENCODING);
		remove_all_header_entries(&mut headers.headers, &CONTENT_LANGUAGE);
		remove_all_header_entries(&mut headers.headers, &CONTENT_LOCATION);
		remove_all_header_entries(&mut headers.headers, &CONTENT_TYPE);
	}

	request.locations.push(location.clone());
	request.url = location;

	let policy = headers.headers.get_all(REFERRER_POLICY).into_iter().rev();
	let policy = policy
		.filter(|v| !v.is_empty())
		.find_map(|v| ReferrerPolicy::from_str(str::from_utf8(v.as_bytes()).unwrap()).ok());
	if let Some(policy) = policy {
		request.referrer_policy = policy;
	}

	main_fetch(cx, request, client, redirections + 1).await
}

pub fn define(cx: &Context, global: &mut Object) -> bool {
	let _ = GLOBAL_CLIENT.set(default_client());
	global.define_method(cx, "fetch", fetch, 1, PropertyFlags::CONSTANT_ENUMERATED);
	Headers::init_class(cx, global).0 && Request::init_class(cx, global).0 && Response::init_class(cx, global).0
}
