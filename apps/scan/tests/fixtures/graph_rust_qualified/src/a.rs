pub mod dentro {
    pub struct Pote(pub usize);

    impl Pote {
        pub fn new(v: usize) -> Self {
            Pote(v)
        }
    }
}

pub struct Caixa<T>(pub T);

impl<T> Caixa<T> {
    pub fn new(v: T) -> Self {
        Caixa(v)
    }
}

pub fn soma() -> usize {
    2
}
