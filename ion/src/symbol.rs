/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::mem::transmute;
use std::ops::{Deref, DerefMut};

use mozjs::jsapi::{
	GetSymbolCode, GetSymbolDescription, GetSymbolFor, GetWellKnownSymbol, NewSymbol, Symbol as JSSymbol,
	SymbolCode as JSSymbolCode,
};

use crate::conversions::{FromValue as _, ToValue as _};
use crate::{Context, Local};

/// Represents a well-known symbol code.
///
/// Each of these refer to a property on the `Symbol` global object.
/// Refer to [MDN](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Symbol#static_properties) for more details.
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
#[repr(u32)]
pub enum WellKnownSymbolCode {
	IsConcatSpreadable,
	Iterator,
	Match,
	Replace,
	Search,
	Species,
	HasInstance,
	Split,
	ToPrimitive,
	ToStringTag,
	Unscopables,
	AsyncIterator,
	MatchAll,
}

/// Represents the code of a [Symbol].
/// The code can be a [WellKnownSymbolCode], a private name symbol, a symbol within the registry, or a unique symbol.
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
pub enum SymbolCode {
	WellKnown(WellKnownSymbolCode),
	PrivateNameSymbol,
	InSymbolRegistry,
	UniqueSymbol,
}

impl WellKnownSymbolCode {
	/// Converts a [WellKnownSymbolCode] into its corresponding identifier.
	/// These identifiers refer to the property names on the `Symbol` global object.
	pub const fn identifier(&self) -> &'static str {
		use WellKnownSymbolCode as WKSC;
		match self {
			WKSC::IsConcatSpreadable => "isConcatSpreadable",
			WKSC::Iterator => "iterator",
			WKSC::Match => "match",
			WKSC::Replace => "replace",
			WKSC::Search => "search",
			WKSC::Species => "species",
			WKSC::HasInstance => "hasInstance",
			WKSC::Split => "split",
			WKSC::ToPrimitive => "toPrimitive",
			WKSC::ToStringTag => "toStringTag",
			WKSC::Unscopables => "unscopables",
			WKSC::AsyncIterator => "asyncIterator",
			WKSC::MatchAll => "matchAll",
		}
	}
}

impl SymbolCode {
	/// Checks if a [SymbolCode] is a well-known symbol code.
	pub fn well_known(&self) -> Option<WellKnownSymbolCode> {
		if let SymbolCode::WellKnown(code) = self {
			Some(*code)
		} else {
			None
		}
	}
}

impl From<JSSymbolCode> for SymbolCode {
	fn from(code: JSSymbolCode) -> SymbolCode {
		if (code as u32) < JSSymbolCode::Limit as u32 {
			SymbolCode::WellKnown(unsafe { transmute::<JSSymbolCode, WellKnownSymbolCode>(code) })
		} else {
			use JSSymbolCode as JSSC;
			match code {
				JSSC::PrivateNameSymbol => SymbolCode::PrivateNameSymbol,
				JSSC::InSymbolRegistry => SymbolCode::InSymbolRegistry,
				JSSC::UniqueSymbol => SymbolCode::UniqueSymbol,
				_ => unreachable!(),
			}
		}
	}
}

impl From<WellKnownSymbolCode> for SymbolCode {
	fn from(code: WellKnownSymbolCode) -> SymbolCode {
		SymbolCode::WellKnown(code)
	}
}

impl From<WellKnownSymbolCode> for JSSymbolCode {
	fn from(code: WellKnownSymbolCode) -> Self {
		unsafe { transmute(code) }
	}
}

impl From<SymbolCode> for JSSymbolCode {
	fn from(code: SymbolCode) -> JSSymbolCode {
		use JSSymbolCode as JSSC;
		match code {
			SymbolCode::WellKnown(code) => code.into(),
			SymbolCode::PrivateNameSymbol => JSSC::PrivateNameSymbol,
			SymbolCode::InSymbolRegistry => JSSC::InSymbolRegistry,
			SymbolCode::UniqueSymbol => JSSC::UniqueSymbol,
		}
	}
}

/// Represents a symbol in the JavaScript Runtime.
/// Refer to [MDN](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Symbol) for more details.
#[derive(Debug)]
pub struct Symbol<'s> {
	sym: Local<'s, *mut JSSymbol>,
}

impl Symbol<'_> {
	/// Creates a new unique symbol with a given description.
	pub fn new<'cx>(cx: &'cx Context, description: &str) -> Symbol<'cx> {
		let description = description.as_value(cx);
		let description = cx.root(description.handle().to_string());

		let symbol = unsafe { NewSymbol(cx.as_ptr(), description.handle().into()) };
		Symbol { sym: cx.root(symbol) }
	}

	/// Gets a [Symbol] from the symbol registry with the given key.
	pub fn for_key<'cx>(cx: &'cx Context, key: &str) -> Symbol<'cx> {
		let key = key.as_value(cx);
		let key = cx.root(key.handle().to_string());

		let symbol = unsafe { GetSymbolFor(cx.as_ptr(), key.handle().into()) };
		Symbol { sym: cx.root(symbol) }
	}

	/// Creates a well-known symbol with its corresponding code.
	pub fn well_known(cx: &Context, code: WellKnownSymbolCode) -> Symbol {
		let symbol = unsafe { GetWellKnownSymbol(cx.as_ptr(), code.into()) };
		Symbol { sym: cx.root(symbol) }
	}

	/// Returns the identifying code of a [Symbol].
	pub fn code(&self) -> SymbolCode {
		unsafe { GetSymbolCode(self.sym.handle().into()).into() }
	}

	/// Returns the description of a [Symbol].
	/// Returns [None] for well-known symbols.
	pub fn description(&self, cx: &Context) -> Option<String> {
		let description = unsafe { GetSymbolDescription(self.sym.handle().into()) };
		if !description.is_null() {
			let description = description.as_value(cx);
			String::from_value(cx, &description, true, ()).ok()
		} else {
			None
		}
	}
}

impl<'o> From<Local<'o, *mut JSSymbol>> for Symbol<'o> {
	fn from(sym: Local<'o, *mut JSSymbol>) -> Symbol<'o> {
		Symbol { sym }
	}
}

impl<'s> Deref for Symbol<'s> {
	type Target = Local<'s, *mut JSSymbol>;

	fn deref(&self) -> &Self::Target {
		&self.sym
	}
}

impl DerefMut for Symbol<'_> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.sym
	}
}

#[cfg(test)]
mod tests {
	use mozjs::jsapi::SymbolCode as JSSymbolCode;

	use crate::symbol::{SymbolCode, WellKnownSymbolCode};

	macro_rules! convert_codes {
		($(($js:expr, $native:expr)$(,)?)*) => {
			$(
				assert_eq!($js, JSSymbolCode::from($native));
				assert_eq!($native, SymbolCode::from($js));
			)*
		}
	}

	#[test]
	fn code_conversion() {
		use {JSSymbolCode as JSSC, SymbolCode as SC, WellKnownSymbolCode as WKSC};

		// Well Known Symbol Codes
		convert_codes! {
			(JSSC::isConcatSpreadable, SC::WellKnown(WKSC::IsConcatSpreadable)),
			(JSSC::iterator, SC::WellKnown(WKSC::Iterator)),
			(JSSC::match_, SC::WellKnown(WKSC::Match)),
			(JSSC::replace, SC::WellKnown(WKSC::Replace)),
			(JSSC::search, SC::WellKnown(WKSC::Search)),
			(JSSC::species, SC::WellKnown(WKSC::Species)),
			(JSSC::hasInstance, SC::WellKnown(WKSC::HasInstance)),
			(JSSC::toPrimitive, SC::WellKnown(WKSC::ToPrimitive)),
			(JSSC::toStringTag, SC::WellKnown(WKSC::ToStringTag)),
			(JSSC::unscopables, SC::WellKnown(WKSC::Unscopables)),
			(JSSC::asyncIterator, SC::WellKnown(WKSC::AsyncIterator)),
			(JSSC::matchAll, SC::WellKnown(WKSC::MatchAll)),
		}

		// Other Symbol Codes
		convert_codes! {
			(JSSC::PrivateNameSymbol, SC::PrivateNameSymbol),
			(JSSC::InSymbolRegistry, SC::InSymbolRegistry),
			(JSSC::UniqueSymbol, SC::UniqueSymbol),
		}
	}
}
