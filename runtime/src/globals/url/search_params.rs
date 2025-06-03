/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use form_urlencoded::{Serializer, parse};
use ion::class::Reflector;
use ion::conversions::{FromValue, ToValue};
use ion::function::Opt;
use ion::symbol::WellKnownSymbolCode;
use ion::{ClassDefinition, Context, Error, ErrorKind, JSIterator, Local, Object, OwnedKey, Result, Value, js_class};
use mozjs::jsapi::{Heap, JSObject};
use url::Url;

use crate::globals::url::URL;

pub struct URLSearchParamsInit(Vec<(String, String)>);

impl<'cx> FromValue<'cx> for URLSearchParamsInit {
	type Config = ();
	fn from_value(cx: &'cx Context, value: &Value, strict: bool, _: ()) -> Result<URLSearchParamsInit> {
		if let Ok(vec) = <Vec<Value>>::from_value(cx, value, strict, ()) {
			let entries = vec
				.iter()
				.map(|value| {
					let vec = <Vec<String>>::from_value(cx, value, strict, ())?;
					let boxed: Box<[String; 2]> = vec
						.try_into()
						.map_err(|_| Error::new("Expected Search Parameter Entry with Length 2", ErrorKind::Type))?;
					let [name, value] = *boxed;
					Ok((name, value))
				})
				.collect::<Result<_>>()?;
			Ok(URLSearchParamsInit(entries))
		} else if let Ok(object) = Object::from_value(cx, value, strict, ()) {
			let vec = object
				.iter(cx, None)
				.filter_map(|(key, value)| {
					let value = match value.and_then(|v| String::from_value(cx, &v, strict, ())) {
						Ok(v) => v,
						Err(e) => return Some(Err(e)),
					};
					match key.to_owned_key(cx) {
						Ok(OwnedKey::Int(i)) => Some(Ok((i.to_string(), value))),
						Ok(OwnedKey::String(key)) => Some(Ok((key, value))),
						Err(e) => Some(Err(e)),
						_ => None,
					}
				})
				.collect::<Result<_>>()?;
			Ok(URLSearchParamsInit(vec))
		} else if let Ok(string) = String::from_value(cx, value, strict, ()) {
			let string = string.strip_prefix('?').unwrap_or(&string);
			Ok(URLSearchParamsInit(parse(string.as_bytes()).into_owned().collect()))
		} else {
			Err(Error::new("Invalid Search Params Initialiser", ErrorKind::Type))
		}
	}
}

#[js_class]
pub struct URLSearchParams {
	reflector: Reflector,
	pub(super) pairs: Vec<(String, String)>,
	url: Option<Heap<*mut JSObject>>,
}

impl URLSearchParams {
	pub(super) fn from_url(url: &Url, url_object: *mut JSObject) -> Box<URLSearchParams> {
		let search_params = Box::new(URLSearchParams {
			reflector: Reflector::default(),
			pairs: url.query_pairs().into_owned().collect(),
			url: Some(Heap::default()),
		});
		if let Some(url) = search_params.url.as_ref() {
			url.set(url_object);
		}
		search_params
	}

	pub(super) fn set_pairs_from_url(&mut self, url: &Url) {
		self.pairs = url.query_pairs().into_owned().collect();
	}

	pub fn pairs(&self) -> &[(String, String)] {
		&self.pairs
	}
}

#[js_class]
impl URLSearchParams {
	#[ion(constructor)]
	pub fn constructor(Opt(init): Opt<URLSearchParamsInit>) -> URLSearchParams {
		let pairs = init.map(|init| init.0).unwrap_or_default();
		URLSearchParams {
			reflector: Reflector::default(),
			pairs,
			url: None,
		}
	}

	#[ion(get)]
	pub fn get_size(&self) -> i32 {
		self.pairs.len() as i32
	}

	pub fn append(&mut self, name: String, value: String) {
		self.pairs.push((name, value));
		self.update();
	}

	pub fn delete(&mut self, name: String, Opt(value): Opt<String>) {
		if let Some(value) = value {
			self.pairs.retain(|(k, v)| k != &name || v != &value)
		} else {
			self.pairs.retain(|(k, _)| k != &name)
		}
		self.update();
	}

	pub fn get(&self, key: String) -> Option<String> {
		self.pairs.iter().find(|(k, _)| k == &key).map(|(_, v)| v.clone())
	}

	#[ion(name = "getAll")]
	pub fn get_all(&self, key: String) -> Vec<String> {
		self.pairs.iter().filter(|(k, _)| k == &key).map(|(_, v)| v.clone()).collect()
	}

	pub fn has(&self, key: String, Opt(value): Opt<String>) -> bool {
		if let Some(value) = value {
			self.pairs.iter().any(|(k, v)| k == &key && v == &value)
		} else {
			self.pairs.iter().any(|(k, _)| k == &key)
		}
	}

	pub fn set(&mut self, key: String, value: String) {
		let mut i = 0;
		let mut index = None;

		self.pairs.retain(|(k, _)| {
			if index.is_none() {
				if k == &key {
					index = Some(i);
				} else {
					i += 1;
				}
				true
			} else {
				k != &key
			}
		});

		match index {
			Some(index) => self.pairs[index].1 = value,
			None => self.pairs.push((key, value)),
		}

		self.update()
	}

	pub fn sort(&mut self) {
		self.pairs.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
		self.update()
	}

	#[ion(name = "toString")]
	#[expect(clippy::inherent_to_string)]
	pub fn to_string(&self) -> String {
		Serializer::new(String::new()).extend_pairs(&*self.pairs).finish()
	}

	fn update(&mut self) {
		if let Some(url) = &self.url {
			let url = Object::from(unsafe { Local::from_heap(url) });
			let url = unsafe { URL::get_mut_private_unchecked(&url) };
			if self.pairs.is_empty() {
				url.url.set_query(None);
			} else {
				url.url.query_pairs_mut().clear().extend_pairs(&self.pairs);
			}
		}
	}

	#[ion(name = WellKnownSymbolCode::Iterator)]
	pub fn iterator(cx: &Context, #[ion(this)] this: &Object) -> ion::Iterator {
		let thisv = this.as_value(cx);
		ion::Iterator::new(SearchParamsIterator::default(), &thisv)
	}
}

#[derive(Default)]
pub struct SearchParamsIterator(usize);

impl JSIterator for SearchParamsIterator {
	fn next_value<'cx>(&mut self, cx: &'cx Context, private: &Value<'cx>) -> Option<Value<'cx>> {
		let object = private.to_object(cx);
		let search_params = URLSearchParams::get_private(cx, &object).unwrap();
		let pair = search_params.pairs.get(self.0);
		pair.map(move |(k, v)| {
			self.0 += 1;
			[k, v].as_value(cx)
		})
	}
}
