/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::collections::HashMap;
use std::fmt;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ion::{Context, ErrorReport, Function, Object, Value};
use mozjs::jsapi::JSFunction;
use mozjs::jsval::JSVal;

pub struct SignalMacrotask {
	callback: Option<Box<dyn FnOnce()>>,
	terminate: Arc<AtomicBool>,
	scheduled: Instant,
}

impl SignalMacrotask {
	pub fn new(callback: Box<dyn FnOnce()>, terminate: Arc<AtomicBool>, duration: Duration) -> SignalMacrotask {
		SignalMacrotask {
			callback: Some(callback),
			terminate,
			scheduled: Instant::now() + duration,
		}
	}
}

impl Debug for SignalMacrotask {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		f.debug_struct("SignalMacrotask")
			.field("callback", &self.callback.as_ref().map(|_| ()))
			.field("terminate", &self.terminate.as_ref())
			.field("scheduled", &self.scheduled)
			.finish()
	}
}

#[derive(Debug)]
pub struct TimerMacrotask {
	callback: *mut JSFunction,
	arguments: Box<[JSVal]>,
	repeat: bool,
	scheduled: Instant,
	duration: Duration,
	nesting: u8,
}

impl TimerMacrotask {
	pub fn new(callback: &Function, arguments: Box<[JSVal]>, repeat: bool, duration: Duration) -> TimerMacrotask {
		TimerMacrotask {
			callback: callback.get(),
			arguments,
			repeat,
			duration,
			scheduled: Instant::now(),
			nesting: 0,
		}
	}

	pub fn reset(&mut self) -> bool {
		if self.repeat {
			self.scheduled = Instant::now();
		}
		self.repeat
	}
}

#[derive(Debug)]
pub struct UserMacrotask {
	callback: *mut JSFunction,
	scheduled: Instant,
}

impl UserMacrotask {
	pub fn new(callback: &Function) -> UserMacrotask {
		UserMacrotask {
			callback: callback.get(),
			scheduled: Instant::now(),
		}
	}
}

#[derive(Debug)]
pub enum Macrotask {
	Signal(SignalMacrotask),
	Timer(TimerMacrotask),
	User(UserMacrotask),
}

#[derive(Debug, Default)]
pub struct MacrotaskQueue {
	pub(crate) map: HashMap<u32, Macrotask>,
	pub(crate) nesting: u8,
	next: Option<u32>,
	latest: Option<u32>,
}

impl Macrotask {
	pub fn run(&mut self, cx: &Context) -> Result<(), Option<ErrorReport>> {
		if let Macrotask::Signal(signal) = self {
			if let Some(callback) = signal.callback.take() {
				callback();
			}
			return Ok(());
		}

		let (callback, args) = match self {
			Macrotask::Timer(timer) => (timer.callback, timer.arguments.clone()),
			Macrotask::User(user) => (user.callback, Box::default()),
			_ => unreachable!(),
		};

		let callback = Function::from(cx.root(callback));
		let args: Vec<_> = args.into_vec().into_iter().map(|value| Value::from(cx.root(value))).collect();

		callback.call(cx, &Object::global(cx), args.as_slice())?;
		Ok(())
	}

	pub fn remove(&mut self) -> bool {
		match self {
			Macrotask::Timer(timer) => !timer.reset(),
			_ => true,
		}
	}

	fn terminate(&self) -> bool {
		match self {
			Macrotask::Signal(signal) => signal.terminate.load(Ordering::SeqCst),
			_ => false,
		}
	}

	fn remaining(&self) -> Duration {
		match self {
			Macrotask::Signal(signal) => signal.scheduled - Instant::now(),
			Macrotask::Timer(timer) => timer.scheduled + timer.duration - Instant::now(),
			Macrotask::User(user) => user.scheduled - Instant::now(),
		}
	}
}

impl MacrotaskQueue {
	pub fn run_job(&mut self, cx: &Context) -> Result<(), Option<ErrorReport>> {
		self.find_next();
		if let Some(next) = self.next {
			{
				let macrotask = self.map.get_mut(&next);
				if let Some(macrotask) = macrotask {
					macrotask.run(cx)?;
				}
			}

			// The previous reference may be invalidated by running the macrotask.
			let macrotask = self.map.get_mut(&next);
			if let Some(macrotask) = macrotask
				&& macrotask.remove()
			{
				self.map.remove(&next);
			}
		}

		Ok(())
	}

	pub fn enqueue(&mut self, mut macrotask: Macrotask, id: Option<u32>) -> u32 {
		let index = id.unwrap_or_else(|| self.latest.map_or(0, |l| l + 1));

		let next = self.next.and_then(|next| self.map.get(&next));
		if let Some(next) = next {
			if macrotask.remaining() < next.remaining() {
				self.set_next(index, &macrotask);
			}
		} else {
			self.set_next(index, &macrotask);
		}

		if let Macrotask::Timer(timer) = &mut macrotask {
			self.nesting += 1;
			timer.nesting = self.nesting;
		}

		self.latest = Some(index);
		self.map.insert(index, macrotask);

		index
	}

	pub fn remove(&mut self, id: u32) {
		if self.map.remove(&id).is_some()
			&& let Some(next) = self.next
			&& next == id
		{
			self.next = None;
		}
	}

	pub fn find_next(&mut self) {
		let mut next: Option<(u32, Duration)> = None;

		self.map.retain(|id, macrotask| {
			if macrotask.terminate() {
				return false;
			}

			let duration = macrotask.remaining();
			if let Some((_, next_duration)) = next {
				if duration < next_duration {
					next = Some((*id, duration));
				}
			} else if duration <= Duration::ZERO {
				next = Some((*id, duration));
			}

			true
		});

		self.next = next.map(|(id, _)| id);
	}

	pub fn set_next(&mut self, index: u32, macrotask: &Macrotask) {
		if macrotask.remaining() <= Duration::ZERO {
			self.next = Some(index);
		}
	}

	pub fn is_empty(&self) -> bool {
		self.map.is_empty()
	}
}
