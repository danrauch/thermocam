
#[derive(Debug)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub fn lerp(color1: Self, color2: Self, fraction: f32) -> Self {
        if fraction < 0.0 {
            return color1;
        }
        if fraction > 1.0 {
            return color2;
        }

        let color1_f = (color1.r as f32, color1.g as f32, color1.b as f32);
        let color2_f = (color2.r as f32, color2.g as f32, color2.b as f32);

        let r = (color2_f.0 - color1_f.0) * fraction + color1_f.0;
        let g = (color2_f.1 - color1_f.1) * fraction + color1_f.1;
        let b = (color2_f.2 - color1_f.2) * fraction + color1_f.2;

        Color {
            r: r as u8,
            g: g as u8,
            b: b as u8,
        }
    }
}
