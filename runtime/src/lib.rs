/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

#![allow(clippy::missing_safety_doc, clippy::module_inception)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

#[macro_use]
extern crate ion;

pub use crate::runtime::*;

pub mod cache;
pub mod config;
pub mod event_loop;
pub mod globals;
pub mod module;
#[cfg(feature = "tokio-promise")]
pub mod promise;
mod runtime;
pub mod typescript;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
