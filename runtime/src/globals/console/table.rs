/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::borrow::Cow;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::iter::{empty, once, repeat_with};

use either::Either;
use indexmap::IndexSet;
use ion::conversions::FromValue as _;
use ion::format::key::{KeyDisplay, format_key};
use ion::format::{Config, format_value};
use ion::{Context, Object, OwnedKey, Result};

fn combine_keys(_: &Context, indexes: IndexSet<i32>, headers: IndexSet<String>) -> IndexSet<OwnedKey> {
	let mut indexes: Vec<i32> = indexes.into_iter().collect();
	indexes.sort_unstable();

	let mut keys: IndexSet<OwnedKey> = indexes.into_iter().map(OwnedKey::Int).collect();
	keys.extend(headers.into_iter().map(OwnedKey::String));
	keys
}

pub(crate) fn sort_keys<'cx, I: IntoIterator<Item = Result<OwnedKey<'cx>>>>(
	cx: &'cx Context, unsorted: I,
) -> ion::Result<IndexSet<OwnedKey<'cx>>> {
	let mut indexes = IndexSet::<i32>::new();
	let mut headers = IndexSet::<String>::new();

	for key in unsorted {
		match key {
			Ok(OwnedKey::Int(index)) => indexes.insert(index),
			Ok(OwnedKey::String(header)) => headers.insert(header),
			Err(e) => return Err(e),
			_ => false,
		};
	}

	Ok(combine_keys(cx, indexes, headers))
}

pub(crate) enum Cell<'cx> {
	Key(KeyDisplay<'cx>),
	String(Cow<'static, str>),
}

impl Display for Cell<'_> {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		match self {
			Cell::Key(key) => key.fmt(f),
			Cell::String(string) => Display::fmt(string, f),
		}
	}
}

pub(crate) fn get_cells<'cx>(
	cx: &'cx Context, object: &Object, rows: &'cx IndexSet<OwnedKey>, columns: &'cx IndexSet<OwnedKey>,
	has_values: bool,
) -> impl IntoIterator<Item = impl IntoIterator<Item = Cell<'cx>>> {
	rows.iter().map(move |row| {
		let Ok(Some(value)) = object.get(cx, row) else {
			return Either::Left(empty());
		};
		let key = Cell::Key(format_key(cx, Config::default(), row));

		if let Ok(object) = Object::from_value(cx, &value, true, ()) {
			let cells = columns.iter().map(move |column| match object.get(cx, column) {
				Ok(Some(val)) => Cell::String(Cow::Owned(
					format_value(cx, Config::default().multiline(false).quoted(true), &val).to_string(),
				)),
				_ => Cell::String(Cow::Borrowed("")),
			});

			let cells = once(key).chain(cells);
			let cells = if has_values {
				Either::Left(cells.chain(once(Cell::String(Cow::Borrowed("")))))
			} else {
				Either::Right(cells)
			};

			Either::Right(Either::Right(cells))
		} else {
			let cells = once(key).chain(repeat_with(|| Cell::String(Cow::Borrowed(""))).take(columns.len()));
			let cells = if has_values {
				Either::Left(cells.chain(once(Cell::String(Cow::Owned(
					format_value(cx, Config::default().multiline(false).quoted(true), &value).to_string(),
				)))))
			} else {
				Either::Right(cells)
			};
			Either::Right(Either::Left(cells))
		}
	})
}
