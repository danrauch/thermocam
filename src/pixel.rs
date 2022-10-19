use core::fmt;

pub struct Pixel {
    pub x: u32,
    pub y: u32,
    pub value: f32,
}

impl fmt::Debug for Pixel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}, {}]: {}", self.x, self.y, self.value)
    }
}