pub fn helper() -> usize {
    1
}

pub struct Widget {
    pub width: usize,
}

impl Widget {
    pub fn area(&self) -> usize {
        self.width
    }
}

pub enum Mode {
    Fast,
    Slow,
}

pub trait Render {
    fn draw(&self) -> usize;
}

pub type Count = usize;
