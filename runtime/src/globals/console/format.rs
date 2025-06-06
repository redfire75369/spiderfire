/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::fmt;
use std::fmt::{Display, Formatter, Write as _};

use ion::conversions::FromValue as _;
use ion::format::{Config as FormatConfig, ValueDisplay, format_value};
use ion::{BigInt, Context, Local, Result, Value};
use mozjs::conversions::ConversionBehavior;

use crate::config::{Config, LogLevel};
use crate::globals::console::INDENTS;

pub(crate) enum FormatArg<'cx> {
	String(String),
	Value { value: ValueDisplay<'cx>, spaced: bool },
}

impl FormatArg<'_> {
	pub(crate) fn spaced(&self) -> bool {
		match self {
			FormatArg::String(_) => false,
			FormatArg::Value { spaced, .. } => *spaced,
		}
	}
}

impl Display for FormatArg<'_> {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		match self {
			FormatArg::String(string) => string.fmt(f),
			FormatArg::Value { value, .. } => value.fmt(f),
		}
	}
}

pub(crate) fn format_args<'cx>(cx: &'cx Context, args: &'cx [Value<'cx>]) -> Vec<FormatArg<'cx>> {
	fn inner<'cx>(cx: &'cx Context, args: &'cx [Value<'cx>]) -> Result<Vec<FormatArg<'cx>>> {
		if args.len() <= 1 || !args[0].get().is_string() {
			return Ok(format_value_args(cx, args.iter()).collect());
		}

		let format = String::from_value(cx, &args[0], true, ())?;

		if format.is_empty() {
			return Ok(format_value_args(cx, args.iter()).collect());
		}

		let mut outputs = Vec::new();
		let mut output = String::with_capacity(format.len());

		let mut args = args.iter().skip(1).peekable();
		let mut index = 0;

		for (base, _) in format.match_indices('%') {
			if base < index {
				continue;
			}

			output.push_str(&format[index..base]);
			index = base + 1;

			match get_ascii_at(&format, index) {
				next @ (Some(b'%') | None) => {
					if next.is_some() || index == format.len() {
						output.push('%');
					}
					index += 1;
				}
				Some(b'0'..=b'9' | b'.' | b'd' | b'i' | b'f') => {
					let arg = args.next().unwrap();
					format_number_arg(cx, arg, &format, &mut index, &mut output)?;
				}
				Some(b's') => {
					let arg = args.next().unwrap();
					index += 1;

					output.push_str(&String::from_value(cx, arg, false, ())?);
				}
				Some(b'o' | b'O') => {
					let arg = args.next().unwrap();
					index += 1;

					outputs.push(FormatArg::String(output));
					output = String::with_capacity(format.len() - index);

					outputs.push(FormatArg::Value {
						value: format_value(cx, FormatConfig::default().indentation(INDENTS.get()), arg),
						spaced: false,
					});
				}
				Some(b'c') => {
					index += 1;
				}
				Some(b) => {
					output.push('%');
					output.push(char::from(b));
					index += 1;
				}
			}

			if args.peek().is_none() {
				break;
			}
		}

		output.push_str(&format[index..]);
		outputs.push(FormatArg::String(output));
		outputs.extend(format_value_args(cx, args));
		Ok(outputs)
	}

	inner(cx, args).unwrap_or_else(|error| {
		if Config::global().log_level >= LogLevel::Warn {
			eprintln!("{}", error.format());
		}
		Vec::new()
	})
}

pub(crate) fn format_value_args<'cx>(
	cx: &'cx Context, args: impl Iterator<Item = &'cx Value<'cx>>,
) -> impl Iterator<Item = FormatArg<'cx>> {
	args.map(|arg| FormatArg::Value {
		value: format_value(cx, FormatConfig::default().indentation(INDENTS.get()), arg),
		spaced: true,
	})
}

pub(crate) fn format_number_arg<'cx>(
	cx: &'cx Context, arg: &Value<'cx>, format: &str, index: &mut usize, output: &mut String,
) -> Result<()> {
	let (w_len, width) = parse_maximum(&format[*index..]).unzip();
	*index += w_len.unwrap_or(0);
	let (p_len, precision) = get_ascii_at(format, *index)
		.filter(|b| *b == b'.')
		.and_then(|_| parse_maximum(&format[*index + 1..]))
		.unzip();
	*index += p_len.map_or(0, |len| len + 1);

	match get_ascii_at(format, *index) {
		Some(b'd' | b'i') => {
			if arg.get().is_symbol() {
				output.push_str("NaN");
			} else if arg.get().is_bigint() {
				let bigint = BigInt::from(unsafe { Local::from_marked(&arg.get().to_bigint()) });
				output.push_str(&bigint.to_string(cx, 10).unwrap().to_owned(cx)?);
			} else {
				write_printf(
					output,
					width,
					precision,
					i32::from_value(cx, arg, false, ConversionBehavior::Default)?,
				)?;
			}
			*index += 1;
		}
		Some(b'f') => {
			if arg.get().is_symbol() {
				output.push_str("NaN");
			} else {
				write_printf(output, width, precision, f64::from_value(cx, arg, false, ())?)?;
			}
			*index += 1;
		}
		_ => output.push_str(&format[(*index - 1)..*index]),
	}

	Ok(())
}
fn get_ascii_at(str: &str, index: usize) -> Option<u8> {
	str.as_bytes().get(index).copied().filter(u8::is_ascii)
}

fn parse_maximum(str: &str) -> Option<(usize, usize)> {
	if str.is_empty() || !str.as_bytes()[0].is_ascii_digit() {
		return None;
	}

	let end = str.bytes().position(|b| !b.is_ascii_digit()).unwrap_or(str.len());
	Some((end, str[..end].parse().unwrap()))
}

fn write_printf<D: Display>(
	output: &mut String, width: Option<usize>, precision: Option<usize>, display: D,
) -> fmt::Result {
	match (width, precision) {
		(Some(width), Some(precision)) => write!(output, "{display:width$.precision$}"),
		(Some(width), None) => write!(output, "{display:width$}"),
		(None, Some(precision)) => write!(output, "{display:.precision$}"),
		(None, None) => write!(output, "{display}"),
	}
}
