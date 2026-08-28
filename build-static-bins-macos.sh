#!/usr/bin/env bash
#
# Use this script to compile the binaries statically linked for MacOS (aarch64-apple-darwin)
# and with logging enabled (controlled by RUST_LOG env var)
#
RUSTFLAGS="-C target-feature=+crt-static" \
    cargo build --release \
    --target aarch64-apple-darwin \
    --features env_logger \
    --features clap \
    --features statsd \
    --bins
