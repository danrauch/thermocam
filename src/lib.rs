pub mod rgb_color;
pub mod temperature_pixel;
pub mod thermo_image_processing;

use image;
use image::imageops::FilterType;

#[cfg(not(target_arch = "arm"))]
use npyz;

use linux_embedded_hal::I2cdev;
use mlx9064x;
use mlx9064x::Mlx90640Driver;
use std::thread::sleep;
use std::time::Duration;

use rgb_color::RgbColor;
use temperature_pixel::TemperaturPixel;
use thermo_image_processing::ThermoImageProcessor;

pub fn get_thermo_image_raw_data(
    use_simulation_data: bool,
    shape: &mut (u32, u32),
    mlx_sensor_data: &mut Vec<f32>,
    sensor: &mut Option<Mlx90640Driver<I2cdev>>,
    period: u64,
) {
    if use_simulation_data {
        get_simulation_data(shape, mlx_sensor_data);
    } else {
        match sensor {
            Some(sensor) => {
                sensor.generate_image_if_ready(mlx_sensor_data).unwrap();
                sleep(Duration::from_millis(period));
                sensor.generate_image_if_ready(mlx_sensor_data).unwrap();
            }
            None => panic!("no sensor available in non-simulation mode"),
        }
    }
}

fn get_simulation_data(shape: &mut (u32, u32), mlx_sensor_data: &mut Vec<f32>) {
    #[cfg(not(target_arch = "arm"))]
    {
        let bytes = std::fs::read("data/flir_f32.npy").unwrap();
        let reader = npyz::NpyFile::new(&bytes[..]).unwrap();
        let shape_vec = reader.shape().to_vec();
        *shape = (shape_vec[0] as u32, shape_vec[1] as u32);
        *mlx_sensor_data = reader.into_vec::<f32>().unwrap();
        sleep(Duration::from_millis(250)); // four fps
    }
}

pub fn process_raw_thermo_image_data(
    mlx_sensor_data: &Vec<f32>,
    shape: (u32, u32),
    settings: &ThermoImageProcessor,
) -> (
    TemperaturPixel,
    TemperaturPixel,
    f32,
    image::ImageBuffer<image::Rgb<u8>, Vec<u8>>,
) {
    let mut img_vec: Vec<u8> = Vec::new();
    let mut max_pixel = TemperaturPixel {
        x: 0,
        y: 0,
        value: settings.manual_scale_min_temp,
    };
    let mut min_pixel = TemperaturPixel {
        x: 0,
        y: 0,
        value: settings.manual_scale_max_temp,
    };
    let mut mean_temperature = 0.0;
    for (i, &temp_in_celsius) in mlx_sensor_data.iter().enumerate() {
        let row = i as u32 / shape.1;
        let col = i as u32 % shape.1;

        if temp_in_celsius <= min_pixel.value {
            min_pixel.value = temp_in_celsius;
            min_pixel.x = col;
            min_pixel.y = row;
        }
        if temp_in_celsius >= max_pixel.value {
            max_pixel.value = temp_in_celsius;
            max_pixel.x = col;
            max_pixel.y = row;
        }
        mean_temperature += temp_in_celsius;
    }
    mean_temperature /= mlx_sensor_data.len() as f32;
    let min_temp;
    let max_temp;
    if !settings.autoscale_enabled {
        min_temp = settings.manual_scale_min_temp;
        max_temp = settings.manual_scale_max_temp;
    } else {
        min_temp = min_pixel.value;
        max_temp = max_pixel.value;
    }
    for &temp_in_celsius in mlx_sensor_data.iter() {
        let fraction = normalize(min_temp, max_temp, temp_in_celsius);
        let interpolated_color = RgbColor::lerp(settings.min_temp_color, settings.max_temp_color, fraction);
        img_vec.extend(interpolated_color.to_vec());
    }
    let img = image::RgbImage::from_raw(shape.1, shape.0, img_vec).unwrap();
    let interpolation_factor = settings.interpolation_factor;
    let mut upscaled_image = image::imageops::resize(
        &img,
        img.width() * interpolation_factor,
        img.height() * interpolation_factor,
        FilterType::Lanczos3,
    );

    let x = min_pixel.x * interpolation_factor + interpolation_factor / 2;
    let y = min_pixel.y * interpolation_factor + interpolation_factor / 2;
    draw_cross_into_image(x, y, RgbColor { r: 0, g: 255, b: 0 }, &mut upscaled_image);

    let x = max_pixel.x * interpolation_factor + interpolation_factor / 2;
    let y = max_pixel.y * interpolation_factor + interpolation_factor / 2;
    draw_cross_into_image(x, y, RgbColor { r: 255, g: 255, b: 255 }, &mut upscaled_image);

    (max_pixel, min_pixel, mean_temperature, upscaled_image)
}

fn normalize(min_temp: f32, max_temp: f32, current_temp: f32) -> f32 {
    (current_temp - min_temp) / (max_temp - min_temp)
}

fn draw_cross_into_image(
    x: u32,
    y: u32,
    color: RgbColor,
    upscaled_image: &mut image::ImageBuffer<image::Rgb<u8>, Vec<u8>>,
) {
    let px = image::Rgb([color.r, color.g, color.b]);
    let img_width = upscaled_image.width();
    let img_height = upscaled_image.height();

    // TODO: switch to 2 loops?
    if x >= 1 {
        upscaled_image.put_pixel(x - 1, y, px);
    }
    if x >= 2 {
        upscaled_image.put_pixel(x - 2, y, px);
    }
    if x < img_width {
        upscaled_image.put_pixel(x + 1, y, px);
    }
    if x < img_width - 1 {
        upscaled_image.put_pixel(x + 2, y, px);
    }
    upscaled_image.put_pixel(x, y, px);
    if y >= 1 {
        upscaled_image.put_pixel(x, y - 1, px);
    }
    if y >= 2 {
        upscaled_image.put_pixel(x, y - 2, px);
    }
    if y < img_height {
        upscaled_image.put_pixel(x, y + 1, px);
    }
    if y < img_height - 1 {
        upscaled_image.put_pixel(x, y + 2, px);
    }
}
