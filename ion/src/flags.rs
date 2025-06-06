/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::fmt::{Display, Formatter};
use std::{fmt, str};

use arrayvec::ArrayVec;
use bitflags::{Flags as _, bitflags};
use mozjs::jsapi::{
	JSITER_FORAWAITOF, JSITER_HIDDEN, JSITER_OWNONLY, JSITER_PRIVATE, JSITER_SYMBOLS, JSITER_SYMBOLSONLY,
	JSPROP_ENUMERATE, JSPROP_PERMANENT, JSPROP_READONLY, JSPROP_RESOLVING, RegExpFlag_DotAll, RegExpFlag_Global,
	RegExpFlag_HasIndices, RegExpFlag_IgnoreCase, RegExpFlag_Multiline, RegExpFlag_Sticky, RegExpFlag_Unicode,
	RegExpFlags as REFlags,
};

bitflags! {
	/// Represents the flags of properties on an [Object](crate::Object)
	#[derive(Clone, Copy, Debug)]
	pub struct PropertyFlags: u16 {
		/// Allows enumeration through `Object.keys()`, `for...in` and other functions.
		/// See [Enumerability of Properties](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Enumerability_and_ownership_of_properties#traversing_object_properties).
		const ENUMERATE = JSPROP_ENUMERATE as u16;
		/// Prevents reassignment of the property.
		const READ_ONLY = JSPROP_READONLY as u16;
		/// Prevents deletion and attribute modification of the property.
		const PERMANENT = JSPROP_PERMANENT as u16;
		const RESOLVING = JSPROP_RESOLVING as u16;

		const CONSTANT = PropertyFlags::READ_ONLY.bits() | PropertyFlags::PERMANENT.bits();
		const CONSTANT_ENUMERATED = PropertyFlags::CONSTANT.bits() | PropertyFlags::ENUMERATE.bits();
	}
}

bitflags! {
	/// Represents the flags when iterating over an [Object](crate::Object).
	#[derive(Clone, Copy, Debug, Default)]
	pub struct IteratorFlags: u32 {
		/// Allows iterating over private properties.
		const PRIVATE = JSITER_PRIVATE;
		/// Disallows iterating over inherited properties.
		const OWN_ONLY = JSITER_OWNONLY;
		/// Allows iteration over non-enumerable properties.
		const HIDDEN = JSITER_HIDDEN;
		/// Allows iteration over symbol keys.
		const SYMBOLS = JSITER_SYMBOLS;
		/// Disallows iteration over string keys.
		const SYMBOLS_ONLY = JSITER_SYMBOLSONLY;
		/// Iteration over async iterable objects and async generators.
		const FOR_AWAIT_OF = JSITER_FORAWAITOF;
	}
}

bitflags! {
	/// Represents the flags of a [RegExp](crate::RegExp) object.
	/// See [Advanced searching with flags](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Guide/Regular_expressions#advanced_searching_with_flags)
	#[derive(Clone, Copy, Debug, Eq, PartialEq)]
	pub struct RegExpFlags: u8 {
		/// Determines if indices are generated for matches. Represents `d` flag.
		/// See [hasIndices](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/RegExp/hasIndices).
		const HAS_INDICES = RegExpFlag_HasIndices;
		/// Determines if search is global. Represents `g` flag.
		/// See [global](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/RegExp/global).
		const GLOBAL = RegExpFlag_Global;
		/// Determines if search ignores case. Represents `i` flag.
		/// See [ignoreCase](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/RegExp/ignoreCase).
		const IGNORE_CASE = RegExpFlag_IgnoreCase;
		/// Determines if `^` and `$` match newline characters. Represents `m` flag.
		/// See [multiline](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/RegExp/multiline).
		const MULTILINE = RegExpFlag_Multiline;
		/// Determines if `.` matches newline characters. Represents `s` flag.
		/// See [dotAll](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/RegExp/dotAll).
		const DOT_ALL = RegExpFlag_DotAll;
		/// Determines if pattern uses unicode semantics. Represents `u` flag.
		/// See [unicode](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/RegExp/unicode).
		const UNICODE = RegExpFlag_Unicode;
		/// Determines if search is continued from current index. Represents `y` flag.
		/// See [sticky](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/RegExp/sticky).
		const STICKY = RegExpFlag_Sticky;
	}
}

impl Display for RegExpFlags {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		let mut flags = ArrayVec::<_, 8>::new();
		for flag in RegExpFlags::FLAGS {
			if self.contains(*flag.value()) {
				match *flag.value() {
					RegExpFlags::HAS_INDICES => flags.push(b'd'),
					RegExpFlags::GLOBAL => flags.push(b'g'),
					RegExpFlags::IGNORE_CASE => flags.push(b'i'),
					RegExpFlags::MULTILINE => flags.push(b'm'),
					RegExpFlags::DOT_ALL => flags.push(b's'),
					RegExpFlags::UNICODE => flags.push(b'u'),
					RegExpFlags::STICKY => flags.push(b'y'),
					_ => (),
				}
			}
		}
		flags.sort_unstable();
		f.write_str(unsafe { str::from_utf8_unchecked(&flags) })
	}
}

impl From<RegExpFlags> for REFlags {
	fn from(flags: RegExpFlags) -> Self {
		REFlags { flags_: flags.bits() }
	}
}

impl From<REFlags> for RegExpFlags {
	fn from(flags: REFlags) -> RegExpFlags {
		RegExpFlags::from_bits_retain(flags.flags_)
	}
}
