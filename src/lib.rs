pub mod rgb_color;
pub mod temperature_pixel;
pub mod thermo_image_processing;

use std::fs::File;
use std::io::Read;

use image;
use image::imageops::FilterType;

use bayer;

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
        get_thermo_simulation_data(shape, mlx_sensor_data);
        sleep(Duration::from_millis(period));
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

fn get_thermo_simulation_data(shape: &mut (u32, u32), mlx_sensor_data: &mut Vec<f32>) {
    #[cfg(not(target_arch = "arm"))]
    {
        let bytes = std::fs::read("data/flir_f32.npy").unwrap();
        let reader = npyz::NpyFile::new(&bytes[..]).unwrap();
        let shape_vec = reader.shape().to_vec();
        *shape = (shape_vec[0] as u32, shape_vec[1] as u32);
        *mlx_sensor_data = reader.into_vec::<f32>().unwrap();
    }
}

pub fn get_camera_simulation_data(sim_data_buffer: &mut [u8; 384000]) {
    let mut f = File::open("data/received_image_data.bin").unwrap();
    f.read(sim_data_buffer).unwrap();
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

pub fn blend_images_of_different_sizes(
    image1: image::DynamicImage,
    image2: image::ImageBuffer<image::Rgb<u8>, Vec<u8>>,
    foreground_alpha: f32,
) -> image::ImageBuffer<image::Rgb<u8>, Vec<u8>> {
    let mut blended_img = image::RgbImage::new(image1.width(), image1.height());
    for ((x, y, pxo), pxi) in blended_img
        .enumerate_pixels_mut()
        .zip(image1.as_rgb8().unwrap().pixels())
    {
        let sample_image2_x = ((x as f32 / image1.width() as f32) * image2.width() as f32) as u32;
        let sample_image2_y = ((y as f32 / image1.height() as f32) * image2.height() as f32) as u32;

        let image2_sample = image2.get_pixel(sample_image2_x, sample_image2_y);

        // luminance greyscale
        let mut input_greyscale = 0.3 * pxi.0[0] as f32 + 0.59 * pxi.0[1] as f32 + 0.11 * pxi.0[2] as f32;
        input_greyscale = clamp_to_u8(input_greyscale);

        let blended_r = (image2_sample.0[0] as f32 * foreground_alpha) + (input_greyscale * (1.0 - foreground_alpha));
        let blended_g = (image2_sample.0[1] as f32 * foreground_alpha) + (input_greyscale * (1.0 - foreground_alpha));
        let blended_b = (image2_sample.0[2] as f32 * foreground_alpha) + (input_greyscale * (1.0 - foreground_alpha));

        pxo[0] = blended_r as u8;
        pxo[1] = blended_g as u8;
        pxo[2] = blended_b as u8;
    }
    blended_img
}

pub fn sgrbg10p_to_rgb(buf: &[u8], camera_image_shape: (u32, u32), cam_rgb_raw_buf: &mut [u8]) {
    // convert 10-bit bayer to 16 bit bayer
    let bayer_in_buf_size = (camera_image_shape.0 * camera_image_shape.1) as f32 * 1.25;
    let mut convert_buf = vec![0u8; bayer_in_buf_size as usize];
    // 10-bit depth
    let mut convert_buf_idx = 0;
    for i in (0..bayer_in_buf_size as usize).step_by(5) {
        // unpack pixels
        let pix1 = (buf[i + 0] as u16) << 2 | ((buf[i + 4] >> 0) & 3) as u16;
        let pix2 = (buf[i + 1] as u16) << 2 | ((buf[i + 4] >> 2) & 3) as u16;
        let pix3 = (buf[i + 2] as u16) << 2 | ((buf[i + 4] >> 4) & 3) as u16;
        let pix4 = (buf[i + 3] as u16) << 2 | ((buf[i + 4] >> 6) & 3) as u16;

        // convert 10-bit values to 8-bit
        convert_buf[convert_buf_idx + 0] = (pix1 as f32 / 1024.0 * 255.0) as u8;
        convert_buf[convert_buf_idx + 1] = (pix2 as f32 / 1024.0 * 255.0) as u8;
        convert_buf[convert_buf_idx + 2] = (pix3 as f32 / 1024.0 * 255.0) as u8;
        convert_buf[convert_buf_idx + 3] = (pix4 as f32 / 1024.0 * 255.0) as u8;

        convert_buf_idx += 4;
    }

    // debayer
    let depth = bayer::RasterDepth::Depth8;
    let mut dst = bayer::RasterMut::new(
        camera_image_shape.0 as usize,
        camera_image_shape.1 as usize,
        depth,
        cam_rgb_raw_buf,
    );
    let cfa = bayer::CFA::GBRG; // SGRBG10P
    let alg = bayer::Demosaic::Linear;

    bayer::run_demosaic(
        &mut convert_buf.as_slice(),
        bayer::BayerDepth::Depth8,
        cfa,
        alg,
        &mut dst,
    )
    .unwrap();
}

pub fn yuyv_to_rgb(yuyv_buffer: &[u8], yuyv_shape: (u32, u32), cam_rgb: &mut [u8]) {
    // from https://gist.github.com/wlhe/fcad2999ceb4a826bd811e9fdb6fe652
    let yuyv_buf_size: usize = yuyv_shape.0 as usize * yuyv_shape.1 as usize * 2;
    let mut rgb_idx_offset = 0;

    for yuyv_idx in (0..yuyv_buf_size).step_by(4) {
        let y = yuyv_buffer[yuyv_idx] as i32; // y0
        let u = yuyv_buffer[yuyv_idx + 1] as i32; // u0
        let v = yuyv_buffer[yuyv_idx + 3] as i32; // v0

        let r = y as f32 + 1.4065 * (v - 128) as f32; // r0
        let g = y as f32 - 0.3455 * (v - 128) as f32 - 0.7169 * (v - 128) as f32; // g0
        let b = y as f32 + 1.1790 * (u - 128) as f32; // b0

        cam_rgb[0 + rgb_idx_offset] = r as u8;
        cam_rgb[1 + rgb_idx_offset] = g as u8;
        cam_rgb[2 + rgb_idx_offset] = b as u8;

        let u = yuyv_buffer[yuyv_idx + 1] as i32; // y1
        let y = yuyv_buffer[yuyv_idx + 2] as i32; // u1
        let v = yuyv_buffer[yuyv_idx + 3] as i32; // v1

        let mut r = y as f32 + 1.4065 * (v - 128) as f32; // r1
        let mut g = y as f32 - 0.3455 * (v - 128) as f32 - 0.7169 * (v - 128) as f32; // g1
        let mut b = y as f32 + 1.1790 * (u - 128) as f32; // b1

        r = clamp_to_u8(r);
        g = clamp_to_u8(g);
        b = clamp_to_u8(b);

        cam_rgb[3 + rgb_idx_offset] = r as u8;
        cam_rgb[4 + rgb_idx_offset] = g as u8;
        cam_rgb[5 + rgb_idx_offset] = b as u8;

        rgb_idx_offset += 6;
    }
}

pub fn yuv420_to_rgb(buf: &[u8], shape: (u32, u32)) -> image::ImageBuffer<image::Rgb<u8>, Vec<u8>> {
    let step: u32 = shape.0;
    let size: usize = shape.0 as usize * shape.1 as usize;
    let mut cam_rgb = vec![0u8; size * 3];
    for y_coo in 0..shape.1 {
        for x_coo in 0..shape.0 {
            let offset = (y_coo * step + x_coo) as usize;
            let y: f32 = buf[offset] as f32;
            let u: f32 = buf[(size as u32 + (y_coo / 2) * (step / 2) + x_coo / 2) as usize] as f32;
            let v: f32 = buf[((size as f32 * 1.125) as u32 + (y_coo / 2) * (step / 2) + x_coo / 2) as usize] as f32;

            let mut r: f32 = y + 1.402 * (v - 128.0);
            let mut g: f32 = y - 0.344 * (u - 128.0) - 0.714 * (v - 128.0);
            let mut b: f32 = y + 1.772 * (u - 128.0);

            r = clamp_to_u8(r);
            g = clamp_to_u8(g);
            b = clamp_to_u8(b);

            cam_rgb[(y_coo * step + x_coo) as usize] = r as u8;
            cam_rgb[(y_coo * step + x_coo + 1) as usize] = g as u8;
            cam_rgb[(y_coo * step + x_coo + 2) as usize] = b as u8;
        }
    }
    let img = image::RgbImage::from_raw(shape.0, shape.1, cam_rgb).unwrap();
    img
}

fn clamp_to_u8(value: f32) -> f32 {
    if value < 0.0 {
        0.0
    } else if value > 255.0 {
        255.0
    } else {
        value
    }
}
