/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::fmt;
use std::fmt::{Display, Formatter, Write as _};

use colored::Colorize as _;
use itoa::Buffer;

use crate::format::{Config, NEWLINE, indent_str};
use crate::typedarray::{ArrayBuffer, TypedArray, TypedArrayElement};

pub fn format_array_buffer<'cx>(cfg: Config, buffer: &'cx ArrayBuffer<'cx>) -> ArrayBufferDisplay<'cx> {
	ArrayBufferDisplay { buffer, cfg }
}

#[must_use]
pub struct ArrayBufferDisplay<'cx> {
	buffer: &'cx ArrayBuffer<'cx>,
	cfg: Config,
}

impl Display for ArrayBufferDisplay<'_> {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		let colour = self.cfg.colours.object;
		"ArrayBuffer {".color(colour).fmt(f)?;

		let vec;
		let bytes = if self.buffer.is_shared() {
			vec = unsafe { self.buffer.as_slice().to_vec() };
			&vec
		} else {
			unsafe { self.buffer.as_slice() }
		};

		let indent = indent_str((self.cfg.indentation + self.cfg.depth + 1) as usize);
		if bytes.len() < 8 {
			f.write_char(' ')?;
		} else {
			f.write_str(NEWLINE)?;
			indent.fmt(f)?;
		}

		write!(
			f,
			"{}{} ",
			"[Uint8Contents]".color(self.cfg.colours.symbol),
			":".color(colour)
		)?;
		f.write_char('<')?;

		for (i, byte) in bytes.iter().enumerate() {
			write!(f, "{byte:02x}")?;

			if i != bytes.len() - 1 {
				f.write_char(' ')?;
			}
		}

		f.write_char('>')?;
		",".color(colour).fmt(f)?;
		if bytes.len() < 8 {
			f.write_char(' ')?;
		} else {
			f.write_str(NEWLINE)?;
			indent.fmt(f)?;
		}

		write!(f, "byteLength: {}", bytes.len())?;
		if bytes.len() < 8 {
			f.write_char(' ')?;
		} else {
			f.write_str(NEWLINE)?;
		}

		"}".color(colour).fmt(f)?;

		Ok(())
	}
}

pub fn format_typed_array<'cx, T: TypedArrayElement>(
	cfg: Config, array: &'cx TypedArray<'cx, T>,
) -> TypedArrayDisplay<'cx, T>
where
	T::Element: Display + Copy,
{
	TypedArrayDisplay { array, cfg }
}

pub struct TypedArrayDisplay<'cx, T: TypedArrayElement>
where
	T::Element: Display + Copy,
{
	array: &'cx TypedArray<'cx, T>,
	cfg: Config,
}

impl<T: TypedArrayElement> Display for TypedArrayDisplay<'_, T>
where
	T::Element: Display + Copy,
{
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		let colour = self.cfg.colours.object;

		let vec;
		let elements = if self.array.is_shared() {
			vec = unsafe { self.array.as_slice().to_vec() };
			&vec
		} else {
			unsafe { self.array.as_slice() }
		};

		let mut buffer = Buffer::new();
		write!(
			f,
			"{}{}{}{}",
			T::NAME.color(colour),
			"(".color(colour),
			buffer.format(elements.len()).color(self.cfg.colours.number),
			") [".color(colour)
		)?;

		if elements.is_empty() {
			return "]".color(colour).fmt(f);
		}

		let indent = indent_str((self.cfg.indentation + self.cfg.depth + 1) as usize);
		f.write_str(NEWLINE)?;
		indent.fmt(f)?;

		for (i, element) in elements.iter().enumerate() {
			element.to_string().color(self.cfg.colours.number).fmt(f)?;
			",".color(colour).fmt(f)?;

			if i != elements.len() - 1 {
				f.write_char(' ')?;
				if (i + 1) % 16 == 0 {
					f.write_str(NEWLINE)?;
					indent.fmt(f)?;
				}
			}
		}

		f.write_str(NEWLINE)?;
		"]".color(colour).fmt(f)?;

		Ok(())
	}
}
