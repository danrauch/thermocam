use std::fs::File;
use std::io::Write;
use std::sync::{Arc, Mutex};

use clap;
use image;
use image::imageops::FilterType;

use linux_embedded_hal::I2cdev;
use mlx9064x;
use mlx9064x::Mlx90640Driver;

use thermocam::rgb_color::RgbColor;
use thermocam::{self, thermo_image_processing::ThermoImageProcessor};

use slint;

const COLOR_BLEND_STEPS: u32 = 150;
const INTERPOLATION_FACTOR: u32 = 10;
const MIN_TEMP: f32 = 18.0;
const MAX_TEMP: f32 = 35.0;
const MIN_TEMP_COLOR: RgbColor = RgbColor { r: 0, g: 0, b: 255 };
const MAX_TEMP_COLOR: RgbColor = RgbColor { r: 255, g: 0, b: 0 };

// use opencv::{highgui, prelude::*, videoio, Result};
use v4l::buffer::Type;
use v4l::io::mmap::Stream;
use v4l::io::traits::CaptureStream;
use v4l::video::Capture;
use v4l::Device;
use v4l::FourCC;

slint::include_modules!();
fn main() -> std::io::Result<()> {
    let (
        use_simulation_data,
        deactivate_autoscale,
        camera_image_width,
        camera_image_height,
        new_fourcc,
        foreground_alpha,
        mode_in,
    ) = parse_cli();

    let thermo_process_settings = Arc::new(Mutex::new(
        ThermoImageProcessor::new(INTERPOLATION_FACTOR)
            .with_autoscale_enabled(!deactivate_autoscale)
            .with_manual_scale_min_temp(MIN_TEMP)
            .with_manual_scale_max_temp(MAX_TEMP)
            .with_min_temp_color(MIN_TEMP_COLOR)
            .with_max_temp_color(MAX_TEMP_COLOR)
            .with_mode(mode_in),
    ));

    let main_window = MainWindow::new();

    // UI callbacks
    let thermo_process_settings_clone = Arc::clone(&thermo_process_settings);
    main_window.on_autoscale_toggled(move |autoscale_enabled: bool| {
        thermo_process_settings_clone.lock().unwrap().autoscale_enabled = autoscale_enabled;
    });
    let thermo_process_settings_clone = Arc::clone(&thermo_process_settings);
    main_window.on_manual_scale_min_temp_decreased(move || {
        thermo_process_settings_clone.lock().unwrap().manual_scale_min_temp -= 1.0;
    });
    let thermo_process_settings_clone = Arc::clone(&thermo_process_settings);
    main_window.on_manual_scale_min_temp_increased(move || {
        thermo_process_settings_clone.lock().unwrap().manual_scale_min_temp += 1.0;
    });
    let thermo_process_settings_clone = Arc::clone(&thermo_process_settings);
    main_window.on_manual_scale_max_temp_decreased(move || {
        thermo_process_settings_clone.lock().unwrap().manual_scale_max_temp -= 1.0;
    });
    let thermo_process_settings_clone = Arc::clone(&thermo_process_settings);
    main_window.on_manual_scale_max_temp_increased(move || {
        thermo_process_settings_clone.lock().unwrap().manual_scale_max_temp += 1.0;
    });
    let thermo_process_settings_clone = Arc::clone(&thermo_process_settings);
    main_window.on_mode_decreased(move || {
        let mut settings = thermo_process_settings_clone.lock().unwrap();
        if settings.mode >= 1 {
            settings.mode -= 1;
        }
    });
    let thermo_process_settings_clone = Arc::clone(&thermo_process_settings);
    main_window.on_mode_increased(move || {
        let mut settings = thermo_process_settings_clone.lock().unwrap();
        if settings.mode < 2 {
            settings.mode += 1;
        }
    });

    // generate and set scale image
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

    // handle dynamic UI stuff
    let handle_weak = main_window.as_weak();
    let thread = std::thread::spawn(move || {
        let mut thermo_image_shape = (32, 32);
        let mut sensor_opt: Option<Mlx90640Driver<I2cdev>> = None;

        let frame_rate_in = mlx9064x::FrameRate::Eight;
        let frame_rate: f32 = frame_rate_in.into();
        let period = ((1.0 / frame_rate) * 1000.0) as u64;
        println!("FPS: {:?} ({:?} ms)", frame_rate, period);

        if !use_simulation_data {
            // A buffer for storing the temperature "image"
            let i2c_bus = I2cdev::new("/dev/i2c-1").expect("/dev/i2c-1 needs to be an I2C controller");
            // Default address for these cameras is 0x33
            let mut sensor = Mlx90640Driver::new(i2c_bus, 0x33).unwrap();

            sensor.set_frame_rate(frame_rate_in).unwrap();
            sensor.set_access_pattern(mlx9064x::AccessPattern::Chess).unwrap();
            thermo_image_shape = (sensor.height() as u32, sensor.width() as u32);
            sensor.synchronize().unwrap();

            sensor_opt = Some(sensor);
        }
        let mut mlx_sensor_data = vec![0f32; thermo_image_shape.0 as usize * thermo_image_shape.1 as usize];

        let mut cam_image_shape = (camera_image_width, camera_image_height);
        let mut cam_data_buffer: &[u8];
        let mut sim_data_buffer: [u8; 384000];
        let mut stream_opt: Option<Stream> = None;

        if !use_simulation_data {
            let mut dev = Device::new(0).expect("Failed to open device");
            let mut fmt = dev.format().expect("Failed to read format");
            cam_image_shape = (fmt.width, fmt.height);
            let fourcc = fmt.fourcc;
            println!("Before change: camera shape {cam_image_shape:?} + {fourcc}");
            fmt.width = camera_image_width;
            fmt.height = camera_image_height;
            let new_fourcc_bytes = new_fourcc.as_bytes().try_into().unwrap();
            fmt.fourcc = FourCC::new(new_fourcc_bytes); // YUYV
            dev.set_format(&fmt).expect("Failed to write format");

            fmt = dev.format().expect("Failed to read format");
            let cam_image_shape = (fmt.width, fmt.height);
            let fourcc = fmt.fourcc;
            println!("After change: camera shape {cam_image_shape:?} + {fourcc}");

            stream_opt =
                Some(Stream::with_buffers(&mut dev, Type::VideoCapture, 4).expect("Failed to create buffer stream"));
        }

        loop {
            if use_simulation_data {
                sim_data_buffer = [0; 384000];
                thermocam::get_camera_simulation_data(&mut sim_data_buffer);
                cam_data_buffer = &sim_data_buffer;
            } else {
                let result = stream_opt.as_mut().unwrap().next().unwrap();
                cam_data_buffer = result.0;
                println!(
                    "Buffer size: {}, seq: {}, timestamp: {}, width: {}, height: {}",
                    cam_data_buffer.len(),
                    result.1.sequence,
                    result.1.timestamp,
                    cam_image_shape.0,
                    cam_image_shape.1,
                );
            }

            if false {
                let mut f = File::create("data/received_image_data.bin").unwrap();
                f.write_all(cam_data_buffer).unwrap();
            }

            let rgb_buffer_size = 3 * cam_image_shape.0 as usize * cam_image_shape.1 as usize;
            let mut cam_rgb_raw_buf = vec![0u8; rgb_buffer_size];

            // decode camera data
            thermocam::sgrbg10p_to_rgb(cam_data_buffer, cam_image_shape, &mut cam_rgb_raw_buf);
            let cam_rgb = image::RgbImage::from_raw(cam_image_shape.0, cam_image_shape.1, cam_rgb_raw_buf).unwrap();

            // flip image horizontally
            let cam_rgb_flipped = image::DynamicImage::ImageRgb8(cam_rgb).fliph();

            thermocam::get_thermo_image_raw_data(
                use_simulation_data,
                &mut thermo_image_shape,
                &mut mlx_sensor_data,
                &mut sensor_opt,
                period,
            );

            let mode;
            let min_pixel;
            let max_pixel;
            let min_manual_scale_temp;
            let max_manual_scale_temp;
            let mean_temperature;
            let upscaled_thermo_image;
            {
                // lock mutex in own scope to reduce time locked
                let thermo_process_settings = thermo_process_settings.lock().unwrap();
                (max_pixel, min_pixel, mean_temperature, upscaled_thermo_image) =
                    thermocam::process_raw_thermo_image_data(
                        &mlx_sensor_data,
                        thermo_image_shape,
                        &thermo_process_settings,
                    );
                if thermo_process_settings.autoscale_enabled {
                    min_manual_scale_temp = min_pixel.value;
                    max_manual_scale_temp = max_pixel.value;
                } else {
                    min_manual_scale_temp = thermo_process_settings.manual_scale_min_temp;
                    max_manual_scale_temp = thermo_process_settings.manual_scale_max_temp;
                }
                mode = thermo_process_settings.mode;
            }

            let displayed_image = match mode {
                0 => {
                    thermocam::blend_images_of_different_sizes(cam_rgb_flipped, upscaled_thermo_image, foreground_alpha)
                }
                1 => cam_rgb_flipped.as_rgb8().unwrap().clone(),
                2 => upscaled_thermo_image,
                _ => panic!("image display mode not supported (choose 0, 1 or 2)"),
            };

            let min_pixel_formatted = format!("Min: {:.2}°C", min_pixel.value);
            let mean_pixel_formatted = format!("Mean: {:.2}°C", mean_temperature);
            let max_pixel_formatted = format!("Max: {:.2}°C", max_pixel.value);

            let min_scale_pixel_formatted = format!("{:.0}°C", min_manual_scale_temp);
            let max_scale_pixel_formatted = format!("{:.0}°C", max_manual_scale_temp);

            let handle_copy = handle_weak.clone();
            slint::invoke_from_event_loop(move || {
                let mw = handle_copy.unwrap();
                let ui_image = slint::Image::from_rgb8(slint::SharedPixelBuffer::clone_from_slice(
                    &displayed_image,
                    displayed_image.width(),
                    displayed_image.height(),
                ));

                mw.set_camera_image(ui_image);

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

fn parse_cli() -> (bool, bool, u32, u32, String, f32, u32) {
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
        .arg(
            clap::Arg::new("camera_image_width")
                .short('w')
                .help("Camera image width")
                .default_value("640")
                .value_parser(clap::value_parser!(u32)),
        )
        .arg(
            clap::Arg::new("camera_image_height")
                .short('c')
                .help("Camera image height")
                .default_value("480")
                .value_parser(clap::value_parser!(u32)),
        )
        .arg(
            clap::Arg::new("fourcc")
                .short('f')
                .default_value("pGAA")
                .value_parser(clap::value_parser!(String)),
        )
        .arg(
            clap::Arg::new("foreground_alpha")
                .short('a')
                .default_value("0.5")
                .value_parser(clap::value_parser!(f32)),
        )
        .arg(
            clap::Arg::new("mode")
                .short('m')
                .default_value("0")
                .value_parser(clap::value_parser!(u32)),
        )
        .get_matches();
    let use_simulation_data = matches.get_flag("simulation_data");
    let deactivate_autoscale = matches.get_flag("deactivate_autoscale");
    let camera_image_width = matches
        .try_get_one::<u32>("camera_image_width")
        .expect("Could not read a camera_image_width value")
        .expect("Could not read a camera_image_width value");
    let camera_image_height = matches
        .try_get_one::<u32>("camera_image_height")
        .expect("Could not read a camera_image_height value")
        .expect("Could not read a camera_image_height value");
    let fourcc = matches
        .try_get_one::<String>("fourcc")
        .expect("Could not read a fourcc")
        .expect("Could not read a fourcc");
    let foreground_alpha = matches
        .try_get_one::<f32>("foreground_alpha")
        .expect("Could not read a foreground_alpha")
        .expect("Could not read a foreground_alpha");
    let mode = matches
        .try_get_one::<u32>("mode")
        .expect("Could not read a mode")
        .expect("Could not read a mode");
    (
        use_simulation_data,
        deactivate_autoscale,
        *camera_image_width,
        *camera_image_height,
        fourcc.clone(),
        *foreground_alpha,
        *mode,
    )
}
