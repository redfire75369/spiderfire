/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use crate::conversions::ToValue;
use crate::{Context, Value};

pub type BoxedIntoValue = Box<dyn for<'cx> IntoValue<'cx>>;

/// Represents types that can be converted to JavaScript [Values](Value) with ownership.
/// Primarily used with dynamic dispatch and [`BoxedIntoValue`].
pub trait IntoValue<'cx> {
	/// Converts `self` into a [`Value`] and stores it in `value`.
	fn into_value(self: Box<Self>, cx: &'cx Context, value: &mut Value);
}

impl<'cx, T: ToValue<'cx>> IntoValue<'cx> for T {
	fn into_value(self: Box<Self>, cx: &'cx Context, value: &mut Value) {
		self.to_value(cx, value)
	}
}
