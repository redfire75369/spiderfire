use ion::FromValue;

#[expect(dead_code)]
#[derive(FromValue)]
#[repr(u8)]
enum Representation {
	Zero = 0,
	One = 1,
	Ten = 10,
}
