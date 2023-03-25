use std::sync::{Arc, Mutex};

use clap;
use image;
use image::imageops::FilterType;

use linux_embedded_hal::I2cdev;
use mlx9064x;
use mlx9064x::Mlx90640Driver;

use thermocam::rgb_color::RgbColor;
use thermocam::{get_image_raw_data, process_raw_image_data, RawImageProcessingSettings};

use slint;

const COLOR_BLEND_STEPS: u32 = 150;
const INTERPOLATION_FACTOR: u32 = 10;
const MIN_TEMP: f32 = 18.0;
const MAX_TEMP: f32 = 35.0;
const MIN_TEMP_COLOR: RgbColor = RgbColor { r: 0, g: 0, b: 255 };
const MAX_TEMP_COLOR: RgbColor = RgbColor { r: 255, g: 0, b: 0 };

use v4l::buffer::Type;
use v4l::io::mmap::Stream;
use v4l::io::traits::CaptureStream;
use v4l::video::Capture;
use v4l::Device;
use v4l::FourCC;
use opencv::{highgui, prelude::*, videoio, Result};

slint::include_modules!();
fn main() -> std::io::Result<()> {
    let (use_simulation_data, deactivate_autoscale) = parse_cli();

    let raw_process_settings = Arc::new(Mutex::new(
        RawImageProcessingSettings::new(INTERPOLATION_FACTOR)
            .with_autoscale_enabled(!deactivate_autoscale)
            .with_manual_scale_min_temp(MIN_TEMP)
            .with_manual_scale_max_temp(MAX_TEMP)
            .with_min_temp_color(MIN_TEMP_COLOR)
            .with_max_temp_color(MAX_TEMP_COLOR),
    ));

    let main_window = MainWindow::new();

    // UI callbacks
    let raw_process_settings_clone = Arc::clone(&raw_process_settings);
    main_window.on_autoscale_toggled(move |autoscale_enabled: bool| {
        raw_process_settings_clone.lock().unwrap().autoscale_enabled = autoscale_enabled;
    });
    let raw_process_settings_clone = Arc::clone(&raw_process_settings);
    main_window.on_manual_scale_min_temp_decreased(move || {
        raw_process_settings_clone.lock().unwrap().manual_scale_min_temp -= 1.0;
    });
    let raw_process_settings_clone = Arc::clone(&raw_process_settings);
    main_window.on_manual_scale_min_temp_increased(move || {
        raw_process_settings_clone.lock().unwrap().manual_scale_min_temp += 1.0;
    });
    let raw_process_settings_clone = Arc::clone(&raw_process_settings);
    main_window.on_manual_scale_max_temp_decreased(move || {
        raw_process_settings_clone.lock().unwrap().manual_scale_max_temp -= 1.0;
    });
    let raw_process_settings_clone = Arc::clone(&raw_process_settings);
    main_window.on_manual_scale_max_temp_increased(move || {
        raw_process_settings_clone.lock().unwrap().manual_scale_max_temp += 1.0;
    });

    let col_buf = RgbColor::discrete_blend(MIN_TEMP_COLOR, MAX_TEMP_COLOR, COLOR_BLEND_STEPS);
    let mut buf: Vec<u8> = Vec::new();
    for c in col_buf.iter().rev() {
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
    let thread = std::thread::spawn(move || {
        // A buffer for storing the temperature "image"
        let i2c_bus = I2cdev::new("/dev/i2c-1").expect("/dev/i2c-1 needs to be an I2C controller");
        // Default address for these cameras is 0x33
        let mut sensor = Mlx90640Driver::new(i2c_bus, 0x33).unwrap();

        let frame_rate_in = mlx9064x::FrameRate::Eight;
        let frame_rate: f32 = frame_rate_in.into();
        let period = ((1.0 / frame_rate) * 1000.0) as u64;
        println!("FPS: {:?} ({:?} ms)", frame_rate, period);
        sensor.set_frame_rate(frame_rate_in).unwrap();
        sensor.set_access_pattern(mlx9064x::AccessPattern::Chess).unwrap();
        let mut shape = (sensor.height() as u32, sensor.width() as u32);
        let mut mlx_sensor_data = vec![0f32; sensor.height() * sensor.width()];
        sensor.synchronize().unwrap();

        let mut dev = Device::new(0).expect("Failed to open device");

        // let mut fmt = dev.format().expect("Failed to read format");
        // fmt.width = 640;
        // fmt.height = 480;
        // fmt.fourcc = FourCC::new(b"P010");
        // dev.set_format(&fmt).expect("Failed to write format");

        // let fmt = dev.format().expect("Failed to read format");
        // println!(
        //     "Width: {}, Height: {}, fourcc: {}", // Width: 1920, Height: 1080, fourcc: YUYV
        //     fmt.width,
        //     fmt.height,
        //     fmt.fourcc,
        // );

        let mut stream = Stream::with_buffers(&mut dev, Type::VideoCapture, 4).expect("Failed to create buffer stream");

        // let (tx, rx) = mpsc::channel();

        loop {
            let (buf, meta) = stream.next().unwrap();
            println!(
                "Buffer size: {}, seq: {}, timestamp: {}",
                buf.len(),
                meta.sequence,
                meta.timestamp
            );

            let cam_rgb = fun_name(buf);

            get_image_raw_data(
                use_simulation_data,
                &mut shape,
                &mut mlx_sensor_data,
                &mut sensor,
                period,
            );

            let min_pixel;
            let max_pixel;
            let min_manual_scale_temp;
            let max_manual_scale_temp;
            let mean_temperature;
            let upscaled_image;
            {
                // lock mutex in own scope to reduce time locked
                let raw_process_settings = raw_process_settings.lock().unwrap();
                (max_pixel, min_pixel, mean_temperature, upscaled_image) =
                    process_raw_image_data(&mlx_sensor_data, shape, &raw_process_settings);
                if raw_process_settings.autoscale_enabled {
                    min_manual_scale_temp = min_pixel.value;
                    max_manual_scale_temp = max_pixel.value;
                } else {
                    min_manual_scale_temp = raw_process_settings.manual_scale_min_temp;
                    max_manual_scale_temp = raw_process_settings.manual_scale_max_temp;
                }
            }

            let min_pixel_formatted = format!("Min: {:.2}°C", min_pixel.value);
            let mean_pixel_formatted = format!("Mean: {:.2}°C", mean_temperature);
            let max_pixel_formatted = format!("Max: {:.2}°C", max_pixel.value);

            let min_scale_pixel_formatted = format!("{:.0}°C", min_manual_scale_temp);
            let max_scale_pixel_formatted = format!("{:.0}°C", max_manual_scale_temp);

            let handle_copy = handle_weak.clone();
            slint::invoke_from_event_loop(move || {
                let mw = handle_copy.unwrap();
                let thermo_image = slint::Image::from_rgb8(slint::SharedPixelBuffer::clone_from_slice(
                    upscaled_image.as_raw(),
                    upscaled_image.width(),
                    upscaled_image.height(),
                ));
                let real_image =
                    slint::Image::from_rgb8(slint::SharedPixelBuffer::clone_from_slice(&cam_rgb, 640, 480));

                mw.set_thermo_image(thermo_image);
                mw.set_real_image(real_image);

                mw.set_min_temp_text(slint::SharedString::from(&min_pixel_formatted));
                mw.set_mean_temp_text(slint::SharedString::from(&mean_pixel_formatted));
                mw.set_max_temp_text(slint::SharedString::from(&max_pixel_formatted));

                mw.set_lower_scale_temp_text(slint::SharedString::from(&min_scale_pixel_formatted));
                mw.set_upper_scale_temp_text(slint::SharedString::from(&max_scale_pixel_formatted));
            })
            .unwrap();
        }
    });

    main_window.run();
    thread.join().unwrap();

    Ok(())
}

fn fun_name(buf: &[u8]) -> image::ImageBuffer<image::Rgb<u8>, Vec<u8>> {
    let step: u32 = 640;
    let size: u32 = 640 * 480;
    let mut cam_rgb = vec![0u8; 640 * 480 * 3];
    for i in 0..480 {
        for j in 0..640 {
            let offset = (i * step + j) as usize;
            let Y: f32 = buf[offset] as f32;
            let U: f32 = 128.0; // buf[(size + (i / 2) * (step / 2) + j / 2) as usize] as f32;
            let V: f32 = 128.0; // buf[((size as f32 * 1.125) as u32 + (i / 2) * (step / 2) + j / 2) as usize] as f32;

            let mut R: f32 = Y + 1.402 * (V - 128.0);
            let mut G: f32 = Y - 0.344 * (U - 128.0) - 0.714 * (V - 128.0);
            let mut B: f32 = Y + 1.772 * (U - 128.0);

            if R < 0.0 {
                R = 0.0;
            }
            if G < 0.0 {
                G = 0.0;
            }
            if B < 0.0 {
                B = 0.0;
            }
            if R > 255.0 {
                R = 255.0;
            }
            if G > 255.0 {
                G = 255.0;
            }
            if B > 255.0 {
                B = 255.0;
            }

            cam_rgb[(i * step + j) as usize] = R as u8;
            cam_rgb[(i * step + j + 1) as usize] = G as u8;
            cam_rgb[(i * step + j + 2) as usize] = B as u8;
        }
    }
    let img = image::RgbImage::from_raw(640, 480, cam_rgb).unwrap();
    img
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
