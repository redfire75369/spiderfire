/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use runtime::config::{Config, CONFIG, LogLevel};
use structopt::StructOpt;
use crate::commands::{repl, run};
use crate::commands::eval;

mod commands;
pub mod evaluate;


#[derive(StructOpt)]
#[structopt(name = "spiderfire", about = "JavaScript Runtime")]
struct Cli {
    #[structopt(subcommand)]
    commands: Option<Commands>
}

#[derive(StructOpt)]
pub enum Commands {
    #[structopt(about="Evaluates a line of JavaScript")]
    Eval {
        #[structopt(required(true), about="Line of JavaScript to be evaluated")]
        source: String
    },

    #[structopt(about="Starts a JavaScript Shell")]
    Repl,

    #[structopt(about="Runs a JavaScript file")]
    Run {
        #[structopt(about="The JavaScript file to run. Default: 'main.js'", required(false), default_value="main.js")]
        path: String,

        #[structopt(about="Sets logging level, Default: ERROR", short, long, required(false), default_value = "error")]
        log_level: String,

        #[structopt(about="Sets logging level to DEBUG.", short, long)]
        debug: bool,

        #[structopt(about="Disables ES Modules Features", short, long)]
        script: bool
    }
}

fn main() {
    let args = Cli::from_args();


    match args.commands {
        Some(Eval { source }) => {
             CONFIG
                .set(Config::default().log_level(LogLevel::Debug).script(true))
                .expect("Config Initialisation Failed");
            eval::eval_source(source);
            println!("{}", source);
        }

        Some(Run { path, log_level, debug, script }) => {
			let mut log_lev = LogLevel::Error;

            if debug {
				log_lev = LogLevel::Debug
			} else  {

			    match log_level.to_uppercase().as_str() {
                    "NONE" => log_lev = LogLevel::None,
				    "INFO" => log_lev = LogLevel::Info,
				    "WARN" => log_lev = LogLevel::Warn,
				    "ERROR" => log_lev = LogLevel::Error,
				    "DEBUG" => log_lev = LogLevel::Debug,
				    _ => panic!("Invalid Logging Level")
                }
            }
            CONFIG
                .set(Config::default().log_level(log_lev).script(script))
                .expect("Config Initialisation Failed");
            run::run(path);

            match log_lev {
                LogLevel::None => println!("none"),
                LogLevel::Info => println!("info"),
                LogLevel::Warn => println!("Warn"),
                LogLevel::Error => println!("Error"),
                LogLevel::Debug => println!("Debug")
            }
        }

        Some(Repl) | None => {
            CONFIG
                .set(Config::default().log_level(LogLevel::Debug).script(true))
                .expect("Config Initialisation Failed");
            repl::start_repl();
            println!("REPL!");
        }

    }

}

