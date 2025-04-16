/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

#![allow(clippy::missing_safety_doc, clippy::module_inception)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

#[macro_use]
extern crate mozjs;

use std::result;

pub use bigint::BigInt;
pub use class::ClassDefinition;
pub use context::{Context, ContextInner};
pub use error::{Error, ErrorKind};
pub use exception::{ErrorReport, Exception, ThrowException};
pub use function::{Arguments, Function};
pub use future::PromiseFuture;
#[cfg(feature = "macros")]
pub use ion_proc::*;
pub use object::*;
pub use root::{Local, TracedHeap};
pub use stack::{Stack, StackRecord};
pub use string::{String, StringRef};
pub use symbol::Symbol;
pub use value::Value;

mod bigint;
pub mod class;
pub mod clone;
mod context;
pub mod conversions;
mod error;
pub mod exception;
pub mod flags;
pub mod format;
pub mod function;
mod future;
pub mod module;
pub mod object;
mod root;
pub mod script;
pub mod spec;
pub mod stack;
pub mod string;
pub mod symbol;
pub mod utils;
mod value;

pub type Result<T> = result::Result<T, Error>;
pub type ResultExc<T> = result::Result<T, Exception>;
