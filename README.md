# thermocam

MLX90640 for RPi in Rust

## Cross-Compile for RPi

### Model 1 | arm (32-bit)

```bash
# add rust target
rustup target add arm-unknown-linux-gnueabihf
# add toolchain to path
export PATH="<path-to-gcc-arm-xx.xx-xxxx.xx-x86_64-arm-linux-gnueabihf/bin:$PATH"
# example (for RPi OS Bullseye)
export PATH="/home/dan/arm-toolchain/gcc-arm-xxxxxxxxxxxxxxx/bin:$PATH"
# build with target specified
cargo build --target arm-unknown-linux-gnueabihf

```

### Model >=3 | aarch64 (64-bit)

```bash
#add rust target
rustup target add aarch64-unknown-linux-gnu
# add toolchain to path
export PATH="<path-to-gcc-arm-xx.xx-xxxx.xx-x86_64-aarch64-none-linux-gnu/bin/":$PATH
# example (for RPi OS Bullseye)
export PATH="/home/dan/arm-toolchain/gcc-arm-10.2-2020.11-x86_64-aarch64-none-linux-gnu/bin:$PATH"
# build with target specified
cargo build --target aarch64-unknown-linux-gnu
```

Further hints here: https://chacin.dev/blog/cross-compiling-rust-for-the-raspberry-pi
