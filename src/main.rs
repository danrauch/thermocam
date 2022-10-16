use clap;
use image;
use image::imageops::FilterType;
use npyz;

use linux_embedded_hal::I2cdev;
use mlx9064x;
use mlx9064x::Mlx90640Driver;
use std::thread::sleep;
use std::time::Duration;

mod color;
use color::Color;

#[derive(Debug)]
struct Pixel {
    x: u32,
    y: u32,
    value: f32,
}

fn main() -> std::io::Result<()> {
    const INTERPOLATION_FACTOR: u32 = 6;
    const MIN_TEMP: f32 = 18.0;
    const MAX_TEMP: f32 = 35.0;
    const MIN_TEMP_COLOR: Color = Color { r: 0, g: 0, b: 255 };
    const MAX_TEMP_COLOR: Color = Color { r: 255, g: 0, b: 0 };

    let (use_simulation_data, deactivate_autoscale) = parse_cli();

    let mut img_vec: Vec<u8> = Vec::new();
    let mut max_pixel = Pixel {
        x: 0,
        y: 0,
        value: MIN_TEMP,
    };
    let mut min_pixel = Pixel {
        x: 0,
        y: 0,
        value: MAX_TEMP,
    };

    let mut mlx_sensor_data;
    let shape;
    if use_simulation_data {
        let bytes = std::fs::read("data/flir_f32.npy")?;
        let reader = npyz::NpyFile::new(&bytes[..])?;
        let shape_vec = reader.shape().to_vec();
        shape = (shape_vec[0] as u32, shape_vec[1] as u32);
        mlx_sensor_data = reader.into_vec::<f32>()?;
    } else {
        let i2c_bus = I2cdev::new("/dev/i2c-1").expect("/dev/i2c-1 needs to be an I2C controller");
        // Default address for these cameras is 0x33
        let mut sensor = Mlx90640Driver::new(i2c_bus, 0x33).unwrap();
        sensor.set_frame_rate(mlx9064x::FrameRate::Half).unwrap();
        sensor
            .set_access_pattern(mlx9064x::AccessPattern::Interleave)
            .unwrap();
        // A buffer for storing the temperature "image"
        shape = (sensor.height() as u32, sensor.width() as u32);
        mlx_sensor_data = vec![0f32; sensor.height() * sensor.width()];
        sleep(Duration::from_millis(1000));
        for _ in 0..2 {
            while !sensor
                .generate_image_if_ready(&mut mlx_sensor_data)
                .unwrap()
            {
                sleep(Duration::from_millis(50));
            }
            sleep(Duration::from_millis(500));
        }
    }

    for (i, &temp_in_celsius) in mlx_sensor_data.iter().enumerate() {
        let row = i as u32 / shape.1;
        let col = i as u32 % shape.1;

        let mut corrected_temp_in_celsius = temp_in_celsius;
        if corrected_temp_in_celsius < -40.0 {
            corrected_temp_in_celsius = 21.0;
        }

        if corrected_temp_in_celsius <= min_pixel.value {
            min_pixel.value = corrected_temp_in_celsius;
            min_pixel.x = col;
            min_pixel.y = row;
        }
        if corrected_temp_in_celsius >= max_pixel.value {
            max_pixel.value = corrected_temp_in_celsius;
            max_pixel.x = col;
            max_pixel.y = row;
        }
    }

    for &temp_in_celsius in mlx_sensor_data.iter() {
        let mut corrected_temp_in_celsius = temp_in_celsius;
        if corrected_temp_in_celsius < -40.0 {
            corrected_temp_in_celsius = 21.0;
        }

        let fraction;
        if deactivate_autoscale {
            fraction = normalize(MIN_TEMP, MAX_TEMP, corrected_temp_in_celsius);
        } else {
            fraction = normalize(min_pixel.value, max_pixel.value, corrected_temp_in_celsius);
        }

        let interpolated_color = Color::lerp(MIN_TEMP_COLOR, MAX_TEMP_COLOR, fraction);
        img_vec.push(interpolated_color.r);
        img_vec.push(interpolated_color.g);
        img_vec.push(interpolated_color.b);
    }

    let img = image::RgbImage::from_raw(shape.1, shape.0, img_vec).unwrap();
    // img = image::imageops::rotate90(&img);

    let upscaled_image = image::imageops::resize(
        &img,
        img.width() * INTERPOLATION_FACTOR,
        img.height() * INTERPOLATION_FACTOR,
        FilterType::Lanczos3,
    );

    img.save("output/converted.png").unwrap();
    upscaled_image
        .save("output/converted_upscaled.png")
        .unwrap();

    println!("{:?}", min_pixel);
    println!("{:?}", max_pixel);

    Ok(())
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
