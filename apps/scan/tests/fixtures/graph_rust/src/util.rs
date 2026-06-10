pub fn helper() -> usize {
    1
}

pub struct Widget {
    pub width: usize,
}

pub enum Mode {
    Fast,
    Slow,
}

pub trait Render {
    fn draw(&self) -> usize;
}

pub type Count = usize;
