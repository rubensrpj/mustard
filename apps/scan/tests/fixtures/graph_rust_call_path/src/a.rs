pub fn valor() -> usize {
    1
}

pub fn soma() -> usize {
    valor() + 1
}

pub mod interno {
    pub fn perto() -> usize {
        super::valor()
    }

    pub fn longe() -> usize {
        super::super::x::valor()
    }
}
