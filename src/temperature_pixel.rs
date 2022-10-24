use core::fmt;

pub struct TemperaturPixel {
    pub x: u32,
    pub y: u32,
    pub value: f32,
}

impl fmt::Debug for TemperaturPixel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[x={}, y={}]: value={}°C", self.x, self.y, self.value)
    }
}
impl fmt::Display for TemperaturPixel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}, {}]: {}°C", self.x, self.y, self.value)
    }
}
