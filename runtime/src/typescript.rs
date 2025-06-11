/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::fmt;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::string::FromUtf8Error;

use swc_core::common::comments::{Comments, SingleThreadedComments};
use swc_core::common::errors::{ColorConfig, Handler};
use swc_core::common::input::StringInput;
use swc_core::common::source_map::DefaultSourceMapGenConfig;
use swc_core::common::sync::Lrc;
use swc_core::common::{BytePos, FileName, GLOBALS, Globals, LineCol, Mark, SourceMap as SwcSourceMap};
use swc_core::ecma::ast::{EsVersion, Pass as _, Program};
use swc_core::ecma::codegen::text_writer::JsWriter;
use swc_core::ecma::codegen::{Config as CodegenConfig, Emitter};
use swc_core::ecma::parser::lexer::Lexer;
use swc_core::ecma::parser::{Parser, Syntax, TsSyntax};
use swc_core::ecma::transforms::base::fixer::fixer;
use swc_core::ecma::transforms::base::hygiene::hygiene;
use swc_core::ecma::transforms::base::resolver;
use swc_core::ecma::transforms::typescript::strip;
use swc_core::ecma::visit::VisitMut as _;
use swc_sourcemap::SourceMap;

use crate::config::Config;

pub fn compile_typescript(filename: &str, source: &str) -> Result<(String, SourceMap), Error> {
	let name = Lrc::new(FileName::Real(PathBuf::from(filename)));

	let source_map: Lrc<SwcSourceMap> = Lrc::default();
	let file = source_map.new_source_file(name, String::from(source));
	let input = StringInput::from(&*file);

	let comments = SingleThreadedComments::default();
	let (handler, mut parser) = initialise_parser(Lrc::clone(&source_map), &comments, input);

	let mut buffer = Vec::new();
	let mut mappings = Vec::new();
	let mut emitter = initialise_emitter(Lrc::clone(&source_map), &comments, &mut buffer, &mut mappings);

	let mut program = if Config::global().script {
		Program::Script(parser.parse_script().map_err(|e| {
			e.into_diagnostic(&handler).emit();
			Error::Parse
		})?)
	} else {
		Program::Module(parser.parse_module().map_err(|e| {
			e.into_diagnostic(&handler).emit();
			Error::Parse
		})?)
	};
	handle_program(&mut program, &mut emitter)?;

	let source_map = source_map.build_source_map(&mappings, None, DefaultSourceMapGenConfig);
	Ok((String::from_utf8(buffer)?, source_map))
}

pub fn handle_program(
	program: &mut Program, emitter: &mut Emitter<JsWriter<&mut Vec<u8>>, SwcSourceMap>,
) -> Result<(), Error> {
	let globals = Globals::default();
	GLOBALS.set(&globals, || {
		let unresolved_mark = Mark::new();
		let top_level_mark = Mark::new();

		resolver(unresolved_mark, top_level_mark, true).visit_mut_program(program);
		strip(unresolved_mark, top_level_mark).process(program);
		hygiene().visit_mut_program(program);
		fixer(emitter.comments).visit_mut_program(program);
	});

	emitter.emit_program(program).map_err(|_| Error::Emission)
}

fn initialise_parser<'a>(
	source_map: Lrc<SwcSourceMap>, comments: &'a dyn Comments, input: StringInput<'a>,
) -> (Handler, Parser<Lexer<'a>>) {
	let handler = Handler::with_tty_emitter(ColorConfig::Auto, true, false, Some(source_map));
	let lexer = Lexer::new(
		Syntax::Typescript(TsSyntax::default()),
		EsVersion::Es2022,
		input,
		Some(comments),
	);
	let mut parser = Parser::new_from(lexer);

	for error in parser.take_errors() {
		error.into_diagnostic(&handler).emit();
	}

	(handler, parser)
}

fn initialise_emitter<'a>(
	source_map: Lrc<SwcSourceMap>, comments: &'a dyn Comments, buffer: &'a mut Vec<u8>,
	mappings: &'a mut Vec<(BytePos, LineCol)>,
) -> Emitter<'a, JsWriter<'a, &'a mut Vec<u8>>, SwcSourceMap> {
	Emitter {
		cfg: CodegenConfig::default().with_target(EsVersion::Es2024),
		cm: Lrc::clone(&source_map),
		comments: Some(comments),
		wr: JsWriter::new(source_map, "\n", buffer, Some(mappings)),
	}
}

#[derive(Debug)]
pub enum Error {
	Parse,
	Emission,
	FromUtf8(FromUtf8Error),
}

impl From<FromUtf8Error> for Error {
	fn from(err: FromUtf8Error) -> Error {
		Error::FromUtf8(err)
	}
}

impl Display for Error {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		match self {
			Error::FromUtf8(err) => f.write_str(&err.to_string()),
			_ => Ok(()),
		}
	}
}
