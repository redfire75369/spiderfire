/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::future::Future;
use std::pin::Pin;
use std::task;
use std::task::Poll;

use futures_channel::mpsc;
use futures_channel::mpsc::Receiver;
use futures_util::Stream;
use mozjs::jsval::JSVal;

use crate::{Context, Promise, Value};

pub struct PromiseFuture(Receiver<Result<JSVal, JSVal>>);

impl PromiseFuture {
	pub fn new(cx: &Context, promise: &Promise) -> PromiseFuture {
		let (rx, tx) = mpsc::channel(1);

		let mut rx1 = rx;
		let mut rx2 = rx1.clone();

		promise.add_reactions(
			cx,
			move |_, value| {
				let _ = rx1.try_send(Ok(value.get()));
				Ok(Value::undefined_handle())
			},
			move |_, value| {
				let _ = rx2.try_send(Err(value.get()));
				Ok(Value::undefined_handle())
			},
		);

		PromiseFuture(tx)
	}
}

impl Future for PromiseFuture {
	type Output = Result<JSVal, JSVal>;

	fn poll(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Result<JSVal, JSVal>> {
		let result = Pin::new(&mut self.0);
		if let Poll::Ready(Some(val)) = result.poll_next(cx) {
			Poll::Ready(val)
		} else {
			Poll::Pending
		}
	}
}
