pub fn valor() -> usize {
    1
}

pub mod interno {
    pub fn perto() -> usize {
        super::valor()
    }

    pub fn longe() -> usize {
        super::super::x::dobro()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soma() {
        assert_eq!(valor() + interno::perto(), 2);
    }
}
