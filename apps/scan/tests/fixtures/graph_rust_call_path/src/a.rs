pub fn value() -> usize {
    1
}

pub fn sum() -> usize {
    value() + 1
}

pub mod inner {
    pub fn near() -> usize {
        super::value()
    }

    pub fn far() -> usize {
        super::super::x::value()
    }
}
