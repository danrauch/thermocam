# thermocam
MLX90640 for RPi in Rust

### Build for RPi (>=3; aarch64)

```bash
# add toolchain to path
export PATH="<path-to-gcc-arm-xx.xx-xxxx.xx-x86_64-aarch64-none-linux-gnu/bin/":$PATH
# example (for RPi OS Bullet)
export PATH="/home/dan/arm-toolchain/gcc-arm-10.2-2020.11-x86_64-aarch64-none-linux-gnu/bin/":$PATH
# build with target specified
cargo build --target aarch64-unknown-linux-gnu
```

Further hints here: https://chacin.dev/blog/cross-compiling-rust-for-the-raspberry-pi/https://chacin.dev/blog/cross-compiling-rust-for-the-raspberry-pi/
