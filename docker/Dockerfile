# syntax=docker/dockerfile:1
FROM rust:1.69-bookworm AS build
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
WORKDIR /usr/src/icarust
RUN apt update &&  apt-get -y install make cmake libprotobuf-dev protobuf-compiler libhdf5-dev libzstd-dev git
# build vbz compression kept just in case I need it later
#RUN git clone https://github.com/nanoporetech/vbz_compression.git && cd vbz_compression && mkdir build && git submodule update --init && cd build && cmake -D CMAKE_BUILD_TYPE=Release -D ENABLE_CONAN=OFF -D ENABLE_PERF_TESTING=OFF -D ENABLE_PYTHON=OFF .. && make -j
COPY ../ ./
RUN cargo build --release

# make release image
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y \
    libhdf5-dev \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/src/icarust/target/release/icarust /usr/local/bin/
COPY --from=build /usr/src/icarust/vbz_plugin /vbz_plugin
EXPOSE 10000
EXPOSE 10001
LABEL org.opencontainers.image.created="26/04/23"
LABEL org.opencontainers.image.source="https://github.com/looselab/Icarust/"
LABEL org.opencontainers.image.authors="Rory Munro <rory.munro@nottingham.ac.uk>"
LABEL org.opencontainers.image.version="0.2"
LABEL org.opencontainers.image.revision="c0d21bd5fb423dca3345d1936d0062d8fb7362b2"
ENTRYPOINT ["icarust", "-c", "/configs/config.ini"]
CMD ["-v", "-s", "/configs/config.toml"]
