/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

use clap::{Parser, Subcommand};
use commands::handle_command;
use tokio::task::LocalSet;

mod commands;
mod evaluate;
mod repl;

#[derive(Parser)]
#[command(name = "spiderfire", about = "JavaScript Runtime")]
pub struct Cli {
	#[command(subcommand)]
	command: Option<Command>,
}

#[derive(Subcommand)]
pub(crate) enum Command {
	#[command(about = "Prints Cache Statistics")]
	Cache {
		#[arg(help = "Clears the Cache", short, long)]
		clear: bool,
	},

	#[command(about = "Evaluates a line of JavaScript")]
	Eval {
		#[arg(help = "Line of JavaScript to be evaluated", required(true))]
		source: String,
	},

	#[command(about = "Starts a JavaScript Shell")]
	Repl,

	#[command(about = "Runs a JavaScript file")]
	Run {
		#[arg(
			help = "The JavaScript file to run, Default: 'main.js'",
			required(false),
			default_value = "main.js"
		)]
		path: String,

		#[arg(
			help = "Sets logging level, Default: ERROR",
			short,
			long,
			required(false),
			default_value = "ERROR"
		)]
		log_level: String,

		#[arg(help = "Sets logging level to DEBUG", short, long)]
		debug: bool,

		#[arg(help = "Disables ES Modules Features", short, long)]
		script: bool,
	},
}

#[tokio::main(flavor = "current_thread")]
pub async fn main() {
	let cli = Cli::parse();

	#[cfg(windows)]
	{
		colored::control::set_virtual_terminal(true).unwrap();
	}

	let local = LocalSet::new();
	local.run_until(handle_command(cli)).await;
}
