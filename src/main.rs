use std::sync::{Arc, Mutex};

use clap;
use image::imageops::FilterType;
use image::{self, DynamicImage, Pixel};


use linux_embedded_hal::I2cdev;
use mlx9064x;
use mlx9064x::Mlx90640Driver;

use bayer;

use thermocam::rgb_color::RgbColor;
use thermocam::{
    get_thermo_image_raw_data, process_raw_thermo_image_data, thermo_image_processing::ThermoImageProcessor,
};

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
    let (use_simulation_data, deactivate_autoscale, camera_image_width, camera_image_height, new_fourcc) = parse_cli();

    let raw_process_settings = Arc::new(Mutex::new(
        ThermoImageProcessor::new(INTERPOLATION_FACTOR)
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

        let mut dev = Device::new(0).expect("Failed to open device");
        let mut fmt = dev.format().expect("Failed to read format");
        let camera_image_shape = (fmt.width, fmt.height);
        let fourcc = fmt.fourcc;
        println!("Before change: Camera shape {camera_image_shape:?} + {fourcc}");
        fmt.width = camera_image_width;
        fmt.height = camera_image_height;
        let new_fourcc_bytes = new_fourcc.as_bytes().try_into().unwrap();
        fmt.fourcc = FourCC::new(new_fourcc_bytes); // YUYV
        dev.set_format(&fmt).expect("Failed to write format");

        fmt = dev.format().expect("Failed to read format");
        let camera_image_shape = (fmt.width, fmt.height);
        let fourcc = fmt.fourcc;
        println!("After change: Camera shape {camera_image_shape:?} + {fourcc}");

        let mut stream = Stream::with_buffers(&mut dev, Type::VideoCapture, 4).expect("Failed to create buffer stream");

        loop {
            let (buf, meta) = stream.next().unwrap();
            println!(
                "Buffer size: {}, seq: {}, timestamp: {}, width: {}, height: {}, format: {}",
                buf.len(),
                meta.sequence,
                meta.timestamp,
                camera_image_shape.0,
                camera_image_shape.1,
                fourcc
            );

            // convert 10-bit bayer to 16 bit bayer
            let in_buf_size = (camera_image_shape.0 * camera_image_shape.1) as f32 * 1.25;
            let mut convert_buf = vec![0u8; in_buf_size as usize]; // 10-bit depth

            let mut convert_buf_idx = 0;
            for i in (0..in_buf_size as usize).step_by(5) {
                let pix1 = (buf[i + 0] << 2 | ((buf[i + 4] >> 0) & 3)) as u16;
                let pix2 = (buf[i + 1] << 2 | ((buf[i + 4] >> 2) & 3)) as u16;
                let pix3 = (buf[i + 2] << 2 | ((buf[i + 4] >> 4) & 3)) as u16;
                let pix4 = (buf[i + 3] << 2 | ((buf[i + 4] >> 6) & 3)) as u16;

                convert_buf[convert_buf_idx + 0] = (pix1 >> 8) as u8 & 0xff;
                convert_buf[convert_buf_idx + 1] = (pix1 & 0xff) as u8;
                convert_buf[convert_buf_idx + 2] = (pix2 >> 8) as u8 & 0xff;
                convert_buf[convert_buf_idx + 3] = (pix2 & 0xff) as u8;
                convert_buf[convert_buf_idx + 4] = (pix3 >> 8) as u8 & 0xff;
                convert_buf[convert_buf_idx + 5] = (pix3 & 0xff) as u8;
                convert_buf[convert_buf_idx + 6] = (pix4 >> 8) as u8 & 0xff;
                convert_buf[convert_buf_idx + 7] = (pix4 & 0xff) as u8;

                convert_buf_idx += 8;
            }

            // debayer
            let bayer_out_buf_size: usize = camera_image_shape.0 as usize * camera_image_shape.1 as usize;
            let mut cam_rgb_buf = vec![0u8; bayer_out_buf_size * 3];
            let depth = bayer::RasterDepth::Depth8;
            let mut dst = bayer::RasterMut::new(
                camera_image_shape.0 as usize,
                camera_image_shape.1 as usize,
                depth,
                &mut cam_rgb_buf,
            );
            let cfa = bayer::CFA::GRBG; // SGRBG10P (tested: GBRG, GRBG, BGGR, RGGB)
            let alg = bayer::Demosaic::Linear;

            bayer::run_demosaic(
                &mut convert_buf.as_slice(),
                bayer::BayerDepth::Depth8,
                cfa,
                alg,
                &mut dst,
            )
            .unwrap();

            // let out_buf_size = camera_image_shape.0 * camera_image_shape.1;
            // let mut out_buf = vec![0u8; 3 * out_buf_size as usize]; // 8-bit depth

            // // convert from RGB u16 to u8 RGB
            // let mut i = 0;
            // for idx in (0..bayer_out_buf_size * 3).step_by(1) {
            //     // idx fringe image
            //     out_buf[i] =  cam_rgb_buf[idx];
            //     i += 1;
            // }

            let cam_rgb = image::RgbImage::from_raw(camera_image_shape.0, camera_image_shape.1, cam_rgb_buf).unwrap();
            cam_rgb.save("final_image.jpg").unwrap();
            get_thermo_image_raw_data(
                use_simulation_data,
                &mut thermo_image_shape,
                &mut mlx_sensor_data,
                &mut sensor_opt,
                period,
            );

            let min_pixel;
            let max_pixel;
            let min_manual_scale_temp;
            let max_manual_scale_temp;
            let mean_temperature;
            let upscaled_thermo_image;
            {
                // lock mutex in own scope to reduce time locked
                let raw_process_settings = raw_process_settings.lock().unwrap();
                (max_pixel, min_pixel, mean_temperature, upscaled_thermo_image) =
                    process_raw_thermo_image_data(&mlx_sensor_data, thermo_image_shape, &raw_process_settings);
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

            let mut blended_img = image::RgbImage::new(cam_rgb.width(), cam_rgb.height());
            for ((x, y, pxo), pxi) in blended_img.enumerate_pixels_mut().zip(cam_rgb.pixels()) {
                let sample_thermo_x =
                    ((x as f32 / cam_rgb.width() as f32) * upscaled_thermo_image.width() as f32) as u32;
                let sample_thermo_y =
                    ((y as f32 / cam_rgb.height() as f32) * upscaled_thermo_image.height() as f32) as u32;

                let thermo_sample = upscaled_thermo_image.get_pixel(sample_thermo_x, sample_thermo_y);
                let foreground_alpha = 0.0;

                let mut input_greyscale = 0.3 * pxi.0[0] as f32 + 0.59 * pxi.0[1] as f32 + 0.11 * pxi.0[2] as f32;
                if input_greyscale < 0.0 {
                    input_greyscale = 255.0
                } else if input_greyscale > 255.0 {
                    input_greyscale = 255.0
                }

                let blended_r = (thermo_sample.0[0] as f32 * foreground_alpha) + (input_greyscale * (1.0 - foreground_alpha));
                let blended_g = (thermo_sample.0[1] as f32 * foreground_alpha) + (input_greyscale * (1.0 - foreground_alpha));
                let blended_b = (thermo_sample.0[2] as f32 * foreground_alpha) + (input_greyscale * (1.0 - foreground_alpha));

                pxo[0] = blended_r as u8;
                pxo[1] = blended_g as u8;
                pxo[2] = blended_b as u8;
            }

            let handle_copy = handle_weak.clone();
            slint::invoke_from_event_loop(move || {
                let mw = handle_copy.unwrap();
                let thermo_image = slint::Image::from_rgb8(slint::SharedPixelBuffer::clone_from_slice(
                    upscaled_thermo_image.as_raw(),
                    upscaled_thermo_image.width(),
                    upscaled_thermo_image.height(),
                ));
                let camera_image = slint::Image::from_rgb8(slint::SharedPixelBuffer::clone_from_slice(
                    &blended_img,
                    camera_image_shape.0,
                    camera_image_shape.1,
                ));

                mw.set_thermo_image(thermo_image);
                mw.set_camera_image(camera_image);

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

fn yuyv_to_rgb(yuyv_buffer: &[u8], yuyv_shape: (u32, u32), cam_rgb: &mut [u8]) {
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

        if r < 0.0 {
            r = 0.0;
        }
        if g < 0.0 {
            g = 0.0;
        }
        if b < 0.0 {
            b = 0.0;
        }
        if r > 255.0 {
            r = 255.0;
        }
        if g > 255.0 {
            g = 255.0;
        }
        if b > 255.0 {
            b = 255.0;
        }

        cam_rgb[3 + rgb_idx_offset] = r as u8;
        cam_rgb[4 + rgb_idx_offset] = g as u8;
        cam_rgb[5 + rgb_idx_offset] = b as u8;

        rgb_idx_offset += 6;
    }
}

/* fn yuv420_to_rgb(buf: &[u8], shape: (u32, u32)) -> image::ImageBuffer<image::Rgb<u8>, Vec<u8>> {
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

            if r < 0.0 {
                r = 0.0;
            }
            if g < 0.0 {
                g = 0.0;
            }
            if b < 0.0 {
                b = 0.0;
            }
            if r > 255.0 {
                r = 255.0;
            }
            if g > 255.0 {
                g = 255.0;
            }
            if b > 255.0 {
                b = 255.0;
            }

            cam_rgb[(y_coo * step + x_coo) as usize] = r as u8;
            cam_rgb[(y_coo * step + x_coo + 1) as usize] = g as u8;
            cam_rgb[(y_coo * step + x_coo + 2) as usize] = b as u8;
        }
        let img = image::RgbImage::from_raw(yuyv_shape.0, yuyv_shape.1, cam_rgb).unwrap();
        img
    }
}
 */

fn parse_cli() -> (bool, bool, u32, u32, String) {
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
                .default_value("YUYV")
                .value_parser(clap::value_parser!(String)),
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
    (
        use_simulation_data,
        deactivate_autoscale,
        *camera_image_width,
        *camera_image_height,
        fourcc.clone(),
    )
}
