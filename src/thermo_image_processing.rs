use crate::rgb_color::RgbColor;

#[derive(Debug, Copy, Clone)]
pub struct ThermoImageProcessor {
    pub interpolation_factor: u32,
    pub autoscale_enabled: bool,
    pub manual_scale_min_temp: f32,
    pub manual_scale_max_temp: f32,
    pub min_temp_color: RgbColor,
    pub max_temp_color: RgbColor,
    pub mode: u32,
}

impl ThermoImageProcessor {
    pub fn new(interpolation_factor: u32) -> Self {
        ThermoImageProcessor {
            interpolation_factor,
            autoscale_enabled: true,
            manual_scale_min_temp: -5.0,
            manual_scale_max_temp: 35.0,
            min_temp_color: RgbColor { r: 0, g: 0, b: 255 },
            max_temp_color: RgbColor { r: 255, g: 0, b: 0 },
            mode: 0,
        }
    }

    pub fn with_autoscale_enabled(mut self, autoscale_enabled: bool) -> Self {
        self.autoscale_enabled = autoscale_enabled;
        self
    }

    pub fn with_manual_scale_min_temp(mut self, manual_scale_min_temp: f32) -> Self {
        self.manual_scale_min_temp = manual_scale_min_temp;
        self
    }

    pub fn with_manual_scale_max_temp(mut self, manual_scale_max_temp: f32) -> Self {
        self.manual_scale_max_temp = manual_scale_max_temp;
        self
    }

    pub fn with_min_temp_color(mut self, min_temp_color: RgbColor) -> Self {
        self.min_temp_color = min_temp_color;
        self
    }

    pub fn with_max_temp_color(mut self, max_temp_color: RgbColor) -> Self {
        self.max_temp_color = max_temp_color;
        self
    }

    pub fn with_mode(mut self, mode: u32) -> Self {
        self.mode = mode;
        self
    }
}
