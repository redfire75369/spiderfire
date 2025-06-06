/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use ion::{ClassDefinition as _, Context, Object};
use readable::{
	ByobReader, ByobRequest, ByteStreamController, CommonController, CommonReader, DefaultController, DefaultReader,
	ReadableStream,
};

pub mod readable;

pub fn define<'cx>(cx: &'cx Context, global: &'cx Object) -> bool {
	let dummy = Object::new(cx);
	ReadableStream::init_class(cx, global).0
		&& CommonController::init_class(cx, &dummy).0
		&& ByteStreamController::init_class(cx, global).0
		&& DefaultController::init_class(cx, global).0
		&& ByobRequest::init_class(cx, global).0
		&& CommonReader::init_class(cx, &dummy).0
		&& DefaultReader::init_class(cx, global).0
		&& ByobReader::init_class(cx, global).0
}
