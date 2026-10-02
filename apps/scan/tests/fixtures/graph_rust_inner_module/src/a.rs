pub fn value() -> usize {
    1
}

pub mod inner {
    pub fn near() -> usize {
        super::value()
    }

    pub fn far() -> usize {
        super::super::x::double()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sum() {
        assert_eq!(value() + inner::near(), 2);
    }
}
