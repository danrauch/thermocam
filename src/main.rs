use clap;
use image;
use image::imageops::FilterType;
use npyz;

use linux_embedded_hal::I2cdev;
use mlx9064x;
use mlx9064x::Mlx90640Driver;
use std::thread::sleep;
use std::time::Duration;

// use raspicam::image::camera_operations::click_image;
// use raspicam::image::settings::{CameraSettings, ImageSettings};
// use std::io::Error;
// use std::process::Output;

mod rgb_color;
use rgb_color::RgbColor;

mod temperature_pixel;
use temperature_pixel::TemperaturPixel;

use slint;

const COLOR_BLEND_STEPS: u32 = 150;
const INTERPOLATION_FACTOR: u32 = 8;
const MIN_TEMP: f32 = 18.0;
const MAX_TEMP: f32 = 35.0;
const MIN_TEMP_COLOR: RgbColor = RgbColor { r: 0, g: 0, b: 255 };
const MAX_TEMP_COLOR: RgbColor = RgbColor { r: 255, g: 0, b: 0 };

slint::include_modules!();
fn main() -> std::io::Result<()> {
    let (use_simulation_data, deactivate_autoscale) = parse_cli();

    // let mut camera_settings: CameraSettings = CameraSettings::default();
    // camera_settings.output = "image.jpg";

    // // Initialize image settings with their default values.
    // let image_settings: ImageSettings = ImageSettings::default();

    // // Capture image using RaspberryPi's camera function.
    // let result: Result<Output, Error> = click_image(camera_settings, image_settings);

    // // Print the resultant output or check the clicked image in the default path[~/raspicam.jpg].
    // println!("{:?}", result);

    let main_window = MainWindow::new();
    let col_buf = RgbColor::discrete_blend(MIN_TEMP_COLOR, MAX_TEMP_COLOR, COLOR_BLEND_STEPS);
    let mut buf: Vec<u8> = Vec::new();
    for c in col_buf {
        buf.extend(c.to_vec());
    }
    let scale_img = image::RgbImage::from_raw(1, COLOR_BLEND_STEPS, buf).unwrap();
    let scale_upscaled_img = image::imageops::resize(&scale_img, 15, scale_img.height(), FilterType::Nearest);
    let scale_image = slint::Image::from_rgb8(slint::SharedPixelBuffer::clone_from_slice(
        scale_upscaled_img.as_raw(),
        scale_upscaled_img.width(),
        scale_upscaled_img.height(),
    ));
    main_window.set_scale_image(scale_image);

    let handle_weak = main_window.as_weak();
    let thread = std::thread::spawn(move || loop {
        let (mlx_sensor_data, shape) = get_image_raw_data(use_simulation_data);

        let (max_pixel, min_pixel, mean_temperature, mut upscaled_image) =
            process_raw_image_data(mlx_sensor_data, shape, deactivate_autoscale);

        let x = min_pixel.x * INTERPOLATION_FACTOR + INTERPOLATION_FACTOR / 2;
        let y = min_pixel.y * INTERPOLATION_FACTOR + INTERPOLATION_FACTOR / 2;
        draw_cross_into_image(x, y, RgbColor { r: 0, g: 255, b: 0 }, &mut upscaled_image);

        let x = max_pixel.x * INTERPOLATION_FACTOR + INTERPOLATION_FACTOR / 2;
        let y = max_pixel.y * INTERPOLATION_FACTOR + INTERPOLATION_FACTOR / 2;
        draw_cross_into_image(x, y, RgbColor { r: 255, g: 255, b: 255 }, &mut upscaled_image);

        scale_upscaled_img.save("output/scale_upscaled_img.png").unwrap();
        upscaled_image.save("output/converted_upscaled.png").unwrap();

        println!("Min Pixel {:?}:", min_pixel);
        println!("Max Pixel {:?}:", max_pixel);
        println!("Mean Temp {:?}:", mean_temperature);

        let min_pixel_formatted = format!("Min: {:.2}°C", min_pixel.value);
        let mean_pixel_formatted = format!("Mean: {:.2}°C", mean_temperature);
        let max_pixel_formatted = format!("Max: {:.2}°C", max_pixel.value);

        let min_scale_pixel_formatted = format!("{:.2}°C", min_pixel.value);
        let max_scale_pixel_formatted = format!("{:.2}°C", max_pixel.value);

        let handle_copy = handle_weak.clone();
        slint::invoke_from_event_loop(move || {
            let mw = handle_copy.unwrap();
            let thermo_image = slint::Image::from_rgb8(slint::SharedPixelBuffer::clone_from_slice(
                upscaled_image.as_raw(),
                upscaled_image.width(),
                upscaled_image.height(),
            ));

            mw.set_thermo_image(thermo_image);

            mw.set_min_temp_text(slint::SharedString::from(&min_pixel_formatted));
            mw.set_mean_temp_text(slint::SharedString::from(&mean_pixel_formatted));
            mw.set_max_temp_text(slint::SharedString::from(&max_pixel_formatted));

            mw.set_lower_scale_temp_text(slint::SharedString::from(&min_scale_pixel_formatted));
            mw.set_upper_scale_temp_text(slint::SharedString::from(&max_scale_pixel_formatted));
        })
        .unwrap();
    });

    main_window.run();
    thread.join().unwrap();

    Ok(())
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

fn process_raw_image_data(
    mlx_sensor_data: Vec<f32>,
    shape: (u32, u32),
    deactivate_autoscale: bool,
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
        value: MIN_TEMP,
    };
    let mut min_pixel = TemperaturPixel {
        x: 0,
        y: 0,
        value: MAX_TEMP,
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
    for &temp_in_celsius in mlx_sensor_data.iter() {
        let min_temp;
        let max_temp;
        if deactivate_autoscale {
            min_temp = MIN_TEMP;
            max_temp = MAX_TEMP;
        } else {
            min_temp = min_pixel.value;
            max_temp = max_pixel.value;
        }

        let fraction = normalize(min_temp, max_temp, temp_in_celsius);
        let interpolated_color = RgbColor::lerp(MIN_TEMP_COLOR, MAX_TEMP_COLOR, fraction);
        img_vec.extend(interpolated_color.to_vec());
    }
    let img = image::RgbImage::from_raw(shape.1, shape.0, img_vec).unwrap();

    let upscaled_image = image::imageops::resize(
        &img,
        img.width() * INTERPOLATION_FACTOR,
        img.height() * INTERPOLATION_FACTOR,
        FilterType::Lanczos3,
    );

    (max_pixel, min_pixel, mean_temperature, upscaled_image)
}

fn get_image_raw_data(use_simulation_data: bool) -> (Vec<f32>, (u32, u32)) {
    let mut mlx_sensor_data;
    let shape;
    if use_simulation_data {
        let bytes = std::fs::read("data/flir_f32.npy").unwrap();
        let reader = npyz::NpyFile::new(&bytes[..]).unwrap();
        let shape_vec = reader.shape().to_vec();
        shape = (shape_vec[0] as u32, shape_vec[1] as u32);
        mlx_sensor_data = reader.into_vec::<f32>().unwrap();
        sleep(Duration::from_millis(250)); // four fps
    } else {
        let i2c_bus = I2cdev::new("/dev/i2c-1").expect("/dev/i2c-1 needs to be an I2C controller");
        // Default address for these cameras is 0x33
        let mut sensor = Mlx90640Driver::new(i2c_bus, 0x33).unwrap();

        let frame_rate_in = mlx9064x::FrameRate::Four;
        let frame_rate: f32 = frame_rate_in.into();
        let period = ((1.0 / frame_rate) * 1000.0) as u64;
        println!("FPS: {:?} ({:?} ms)", frame_rate, period);
        sensor.set_frame_rate(frame_rate_in).unwrap();
        sensor.set_access_pattern(mlx9064x::AccessPattern::Interleave).unwrap();
        // A buffer for storing the temperature "image"
        shape = (sensor.height() as u32, sensor.width() as u32);
        mlx_sensor_data = vec![0f32; sensor.height() * sensor.width()];
        sensor.synchronize().unwrap();

        sensor.generate_image_if_ready(&mut mlx_sensor_data).unwrap();
        sleep(Duration::from_millis(period));
        sensor.generate_image_if_ready(&mut mlx_sensor_data).unwrap();

        println!("Ambient temperature: {:?}°C", sensor.ambient_temperature().unwrap());
    }
    (mlx_sensor_data, shape)
}

fn parse_cli() -> (bool, bool) {
    let matches = clap::Command::new("thermocam")
        .arg(
            clap::Arg::new("deactivate_autoscale")
                .short('d')
                .help("Deactivate autoscale")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("simulation_data")
                .short('s')
                .help("Use simulation data")
                .action(clap::ArgAction::SetTrue),
        )
        .get_matches();
    let use_simulation_data = matches.get_flag("simulation_data");
    let deactivate_autoscale = matches.get_flag("deactivate_autoscale");
    (use_simulation_data, deactivate_autoscale)
}

fn normalize(min_temp: f32, max_temp: f32, current_temp: f32) -> f32 {
    (current_temp - min_temp) / (max_temp - min_temp)
}
