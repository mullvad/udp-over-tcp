#!/usr/bin/env bash
#
# Use this script to compile the binaries statically linked for Linux
# and with logging enabled (controlled by RUST_LOG env var)
#
# You need the static version of glibc installed for this to work.
# On Fedora/RHEL that's: glibc-static.
# On Debian/Ubuntu that's: libc6-dev.

RUSTFLAGS="-C target-feature=+crt-static" \
    cargo build --release \
    --target x86_64-unknown-linux-gnu \
    --features env_logger \
    --features clap \
    --bins
