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

### OpenCV Error

apt install libstdc++-12-dev


### Startup

scp target/armv7-unknown-linux-gnueabihf/release/thermocam dan@192.168.178.50:/usr/bin/thermocam
scp dan@192.168.178.50:/home/dan/Bilder/test.jpg .
scp dan@192.168.178.50:/home/dan/Bilder/test.avi .
Add startx /usr/bin/thermocam to .bashrc1^

1296x972 pGAA works (SGRBG10P) 
640x480 pGAA works (SGRBG10P) -> buffer size 384000 => 10bit raw bayer packed, 5 bytes for every 4 pixels
YUV420
648x486