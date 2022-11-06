use std::sync::{Arc, Mutex};

use clap;
use image;
use image::imageops::FilterType;

use thermocam::rgb_color::RgbColor;
use thermocam::{get_image_raw_data, process_raw_image_data, RawImageProcessingSettings};

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

    let raw_process_settings = Arc::new(Mutex::new(
        RawImageProcessingSettings::new(INTERPOLATION_FACTOR)
            .with_autoscale_enabled(!deactivate_autoscale)
            .with_manual_scale_min_temp(MIN_TEMP)
            .with_manual_scale_max_temp(MAX_TEMP)
            .with_min_temp_color(MIN_TEMP_COLOR)
            .with_max_temp_color(MAX_TEMP_COLOR),
    ));

    let main_window = MainWindow::new();
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
                process_raw_image_data(mlx_sensor_data, shape, &raw_process_settings);
            if raw_process_settings.autoscale_enabled {
                min_manual_scale_temp = min_pixel.value;
                max_manual_scale_temp = max_pixel.value;
            } else {
                min_manual_scale_temp = raw_process_settings.manual_scale_min_temp;
                max_manual_scale_temp = raw_process_settings.manual_scale_max_temp;
            }
        }

        //scale_upscaled_img.save("output/scale_upscaled_img.png").unwrap();
        //upscaled_image.save("output/converted_upscaled.png").unwrap();

        // println!("Min Pixel {:?}:", min_pixel);
        // println!("Max Pixel {:?}:", max_pixel);
        // println!("Mean Temp {:?}:", mean_temperature);

        let min_pixel_formatted = format!("Min: {:.2}°C", min_pixel.value);
        let mean_pixel_formatted = format!("Mean: {:.2}°C", mean_temperature);
        let max_pixel_formatted = format!("Max: {:.2}°C", max_pixel.value);

        let min_scale_pixel_formatted = format!("{:.2}°C", min_manual_scale_temp);
        let max_scale_pixel_formatted = format!("{:.2}°C", max_manual_scale_temp);

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
