/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use runtime::cache::Cache;
use runtime::config::{CONFIG, Config, LogLevel};

use crate::{Cli, Command};

mod cache;
mod eval;
mod repl;
mod run;

pub(crate) async fn handle_command(cli: Cli) {
	match cli.command {
		Some(Command::Cache { clear }) => {
			if !clear {
				cache::cache_statistics();
			} else if let Some(cache) = Cache::new()
				&& let Err(err) = cache.clear()
			{
				eprintln!("{err}");
			}
		}

		Some(Command::Eval { source }) => {
			CONFIG.set(Config::default().log_level(LogLevel::Debug).script(true)).unwrap();
			eval::eval_source(&source).await;
		}

		Some(Command::Run { path, log_level, debug, script }) => {
			let log_level = if debug {
				LogLevel::Debug
			} else {
				match log_level.to_uppercase().as_str() {
					"NONE" => LogLevel::None,
					"INFO" => LogLevel::Info,
					"WARN" => LogLevel::Warn,
					"ERROR" => LogLevel::Error,
					"DEBUG" => LogLevel::Debug,
					_ => panic!("Invalid Logging Level"),
				}
			};

			CONFIG.set(Config::default().log_level(log_level).script(script)).unwrap();
			run::run(&path).await;
		}

		Some(Command::Repl) | None => {
			CONFIG.set(Config::default().log_level(LogLevel::Debug).script(true)).unwrap();
			repl::start_repl().await;
		}
	}
}
