/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::future::Future;
use std::ops::{Deref, DerefMut};

use futures_executor::block_on;
use mozjs::gc::HandleObject;
use mozjs::glue::JS_GetPromiseResult;
use mozjs::jsapi::{
	AddPromiseReactions, CallOriginalPromiseReject, CallOriginalPromiseResolve, GetPromiseID, GetPromiseState,
	IsPromiseObject, JSObject, NewPromiseObject, PromiseState, RejectPromise, ResolvePromise,
};

use crate::conversions::ToValue;
use crate::flags::PropertyFlags;
use crate::{Context, Error, Function, Local, Object, ResultExc, Value};

/// Represents a [Promise] in the JavaScript Runtime.
/// Refer to [MDN](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Promise) for more details.
#[derive(Debug)]
pub struct Promise<'p> {
	promise: Local<'p, *mut JSObject>,
}

impl<'p> Promise<'p> {
	/// Creates a new [Promise] which never resolves.
	pub fn new(cx: &'p Context) -> Promise<'p> {
		Promise {
			promise: cx.root(unsafe { NewPromiseObject(cx.as_ptr(), HandleObject::null().into()) }),
		}
	}

	/// Creates a new [Promise] with an executor.
	/// The executor is a function that takes in two functions, `resolve` and `reject`.
	/// `resolve` and `reject` can be called with a [Value] to resolve or reject the promise with the given value.
	pub fn with_executor<F>(cx: &'p Context, executor: F) -> Option<Promise<'p>>
	where
		F: for<'cx> FnOnce(&'cx Context, Function<'cx>, Function<'cx>) -> crate::Result<()> + 'static,
	{
		unsafe {
			let function = Function::from_closure_once(
				cx,
				c"executor",
				Box::new(move |args| {
					let cx = args.cx();
					let resolve_obj = args.value(0).unwrap().to_object(cx).into_local();
					let reject_obj = args.value(1).unwrap().to_object(cx).into_local();
					let resolve = Function::from_object(cx, &resolve_obj).unwrap();
					let reject = Function::from_object(cx, &reject_obj).unwrap();

					match executor(cx, resolve, reject) {
						Ok(()) => Ok(Value::undefined_handle()),
						Err(e) => Err(e.into()),
					}
				}),
				2,
				PropertyFlags::empty(),
			);
			let executor = function.to_object(cx);
			let promise = NewPromiseObject(cx.as_ptr(), executor.handle().into());

			if !promise.is_null() {
				Some(Promise { promise: cx.root(promise) })
			} else {
				None
			}
		}
	}

	/// Creates a new [Promise] with a [Future].
	/// The future is run to completion on the current thread and cannot interact with an asynchronous runtime.
	///
	/// The [Result] of the future determines if the promise is resolved or rejected.
	pub fn block_on_future<F, Output, Error>(cx: &'p Context, future: F) -> Option<Promise<'p>>
	where
		F: Future<Output = Result<Output, Error>> + 'static,
		Output: for<'cx> ToValue<'cx> + 'static,
		Error: for<'cx> ToValue<'cx> + 'static,
	{
		Promise::with_executor(cx, move |cx, resolve, reject| {
			let null = Object::null(cx);
			block_on(async move {
				match future.await {
					Ok(output) => {
						let value = output.as_value(cx);
						if let Err(Some(error)) = resolve.call(cx, &null, &[value]) {
							println!("{}", error.format(cx));
						}
					}
					Err(error) => {
						let value = error.as_value(cx);
						if let Err(Some(error)) = reject.call(cx, &null, &[value]) {
							println!("{}", error.format(cx));
						}
					}
				}
			});
			Ok(())
		})
	}

	/// Creates a new [Promise], that is resolved to the given value.
	/// Similar to `Promise.resolve`
	pub fn resolved(cx: &'p Context, value: &Value) -> Promise<'p> {
		Promise {
			promise: cx.root(unsafe { CallOriginalPromiseResolve(cx.as_ptr(), value.handle().into()) }),
		}
	}

	/// Creates a new [Promise], that is rejected to the given value.
	/// Similar to `Promise.reject`
	pub fn rejected(cx: &'p Context, value: &Value) -> Promise<'p> {
		Promise {
			promise: cx.root(unsafe { CallOriginalPromiseReject(cx.as_ptr(), value.handle().into()) }),
		}
	}

	/// Creates a [Promise] from an object.
	pub fn from(object: Local<'p, *mut JSObject>) -> Option<Promise<'p>> {
		if Promise::is_promise(&object) {
			Some(Promise { promise: object })
		} else {
			None
		}
	}

	/// Creates a [Promise] from aj object
	///
	/// ### Safety
	/// Object must be a Promise.
	pub unsafe fn from_unchecked(object: Local<'p, *mut JSObject>) -> Promise<'p> {
		Promise { promise: object }
	}

	/// Returns the ID of the [Promise].
	pub fn id(&self) -> u64 {
		unsafe { GetPromiseID(self.handle().into()) }
	}

	/// Returns the state of the [Promise].
	///
	/// The state can be `Pending`, `Fulfilled` and `Rejected`.
	pub fn state(&self) -> PromiseState {
		unsafe { GetPromiseState(self.handle().into()) }
	}

	/// Returns the result of the [Promise].
	pub fn result<'cx>(&self, cx: &'cx Context) -> Value<'cx> {
		let mut value = Value::undefined(cx);
		unsafe { JS_GetPromiseResult(self.handle().into(), value.handle_mut().into()) }
		value
	}

	pub fn add_reactions<'cx, T, C>(&self, cx: &'cx Context, on_resolved: T, on_rejected: C) -> bool
	where
		T: for<'cx2> FnOnce(&'cx2 Context, &Value<'cx2>) -> ResultExc<Value<'cx2>> + 'static,
		C: for<'cx2> FnOnce(&'cx2 Context, &Value<'cx2>) -> ResultExc<Value<'cx2>> + 'static,
	{
		let on_resolved = wrap_reaction(cx, on_resolved);
		let on_rejected = wrap_reaction(cx, on_rejected);

		unsafe {
			AddPromiseReactions(
				cx.as_ptr(),
				self.handle().into(),
				on_resolved.handle().into(),
				on_rejected.handle().into(),
			)
		}
	}

	pub fn then<'cx, F>(&self, cx: &'cx Context, on_resolved: F) -> bool
	where
		F: for<'cx2> FnOnce(&'cx2 Context, &Value<'cx2>) -> ResultExc<Value<'cx2>> + 'static,
	{
		let on_resolved = wrap_reaction(cx, on_resolved);

		unsafe {
			AddPromiseReactions(
				cx.as_ptr(),
				self.handle().into(),
				on_resolved.handle().into(),
				HandleObject::null().into(),
			)
		}
	}

	pub fn catch<'cx, F>(&self, cx: &'cx Context, on_rejected: F) -> bool
	where
		F: for<'cx2> FnOnce(&'cx2 Context, &Value<'cx2>) -> ResultExc<Value<'cx2>> + 'static,
	{
		let on_rejected = wrap_reaction(cx, on_rejected);

		unsafe {
			AddPromiseReactions(
				cx.as_ptr(),
				self.handle().into(),
				HandleObject::null().into(),
				on_rejected.handle().into(),
			)
		}
	}

	/// Adds Reactions to the [Promise]
	///
	/// `on_resolved` is similar to calling `.then()` on a promise.
	///
	/// `on_rejected` is similar to calling `.catch()` on a promise.
	pub fn add_reactions_native(
		&self, cx: &'_ Context, on_resolved: Option<Function<'_>>, on_rejected: Option<Function<'_>>,
	) -> bool {
		let mut resolved = Object::null(cx);
		let mut rejected = Object::null(cx);
		if let Some(on_resolved) = on_resolved {
			resolved.handle_mut().set(on_resolved.to_object(cx).handle().get());
		}
		if let Some(on_rejected) = on_rejected {
			rejected.handle_mut().set(on_rejected.to_object(cx).handle().get());
		}
		unsafe {
			AddPromiseReactions(
				cx.as_ptr(),
				self.handle().into(),
				resolved.handle().into(),
				rejected.handle().into(),
			)
		}
	}

	/// Resolves the [Promise] with the given [Value].
	pub fn resolve(&self, cx: &Context, value: &Value) -> bool {
		unsafe { ResolvePromise(cx.as_ptr(), self.handle().into(), value.handle().into()) }
	}

	/// Rejects the [Promise] with the given [Value].
	pub fn reject(&self, cx: &Context, value: &Value) -> bool {
		unsafe { RejectPromise(cx.as_ptr(), self.handle().into(), value.handle().into()) }
	}

	/// Rejects the [Promise] with the given [Error].
	pub fn reject_with_error(&self, cx: &Context, error: &Error) -> bool {
		self.reject(cx, &error.as_value(cx))
	}

	/// Checks if a [*mut] [JSObject] is a promise.
	pub fn is_promise_raw(cx: &Context, object: *mut JSObject) -> bool {
		rooted!(in(cx.as_ptr()) let object = object);
		unsafe { IsPromiseObject(object.handle().into()) }
	}

	/// Checks if an object is a promise.
	pub fn is_promise(object: &Local<*mut JSObject>) -> bool {
		unsafe { IsPromiseObject(object.handle().into()) }
	}
}

impl<'p> Deref for Promise<'p> {
	type Target = Local<'p, *mut JSObject>;

	fn deref(&self) -> &Self::Target {
		&self.promise
	}
}

impl DerefMut for Promise<'_> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.promise
	}
}

fn wrap_reaction<'cx, F>(cx: &'cx Context, reaction: F) -> Object<'cx>
where
	F: for<'cx2> FnOnce(&'cx2 Context, &Value<'cx2>) -> ResultExc<Value<'cx2>> + 'static,
{
	Function::from_closure_once(
		cx,
		c"",
		Box::new(move |args| {
			let value = args.value(0).unwrap();
			reaction(args.cx(), &value)
		}),
		1,
		PropertyFlags::empty(),
	)
	.to_object(cx)
}
