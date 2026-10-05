pub mod inside {
    pub struct Jar(pub usize);

    impl Jar {
        pub fn new(v: usize) -> Self {
            Jar(v)
        }
    }
}

pub struct Boxed<T>(pub T);

impl<T> Boxed<T> {
    pub fn new(v: T) -> Self {
        Boxed(v)
    }
}

pub fn sum() -> usize {
    2
}
