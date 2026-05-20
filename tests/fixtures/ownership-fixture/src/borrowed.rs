//! Types that borrow data from a caller.

pub struct StrSlice<'a> {
    pub data: &'a str,
}

pub struct View<'a, T> {
    slice: &'a [T],
}
