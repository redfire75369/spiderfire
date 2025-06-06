use ion::{Array, Context, Object, Result, ResultExc, js_fn};

#[js_fn]
fn output_empty() {}

#[js_fn]
fn output_regular() -> i8 {
	0
}

#[js_fn]
fn output_object(cx: &Context) -> Object {
	Object::new(cx)
}

#[js_fn]
#[expect(clippy::unnecessary_wraps)]
fn output_result_empty() -> Result<()> {
	Ok(())
}

#[js_fn]
#[expect(clippy::unnecessary_wraps)]
fn output_result_regular() -> Result<f64> {
	Ok(f64::EPSILON)
}

#[js_fn]
#[expect(clippy::unnecessary_wraps)]
fn output_result_array(cx: &Context) -> Result<Array> {
	Ok(Array::new_with_length(cx, 8))
}

#[js_fn]
#[expect(clippy::unnecessary_wraps)]
fn output_result_exception() -> ResultExc<()> {
	Ok(())
}
