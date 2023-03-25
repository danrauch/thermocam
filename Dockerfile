FROM ghcr.io/slint-ui/slint/armv7-unknown-linux-gnueabihf:latest

RUN apt-get install --assume-yes libv4l-dev:armhf ninja-build:armhf

# cross compile opencv, see https://docs.opencv.org/4.x/d0/d76/tutorial_arm_crosscompile_with_cmake.html
# necessary due to installing via apt as armhf does not work
RUN git clone --depth 1 --branch '4.5.1' https://github.com/opencv/opencv.git
RUN cd opencv/platforms/linux && \
    mkdir build && \
    cd build && \
    cmake -DBUILD_PERF_TESTS=OFF -DBUILD_TESTS=OFF -DBUILD_EXAMPLES=OFF -DCMAKE_BUILD_TYPE=Release -DOPENCV_GENERATE_PKGCONFIG=ON -DCMAKE_TOOLCHAIN_FILE=../arm-gnueabi.toolchain.cmake ../../.. && \
    make -j 16 && \
    make install

ENV CMAKE_PREFIX_PATH="/opencv/platforms/linux/build/install"
# ENV OPENCV_LINK_LIBS="/opencv/platforms/linux/build/install/lib"
ENV OPENCV_LINK_PATHS="/opencv/platforms/linux/build/install/lib"
ENV OPENCV_INCLUDE_PATHS="/opencv/platforms/linux/build/install/include"
ENV PKG_CONFIG_PATH="/opencv/platforms/linux/build/install/lib/pkgconfig/opencv4.pc"
ENV OpenCV_DIR="/opencv/platforms/linux/build/install/lib/cmake/opencv4"