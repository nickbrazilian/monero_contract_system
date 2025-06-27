# Single-stage Dockerfile with only one apt-get command
FROM rust:1.80-bookworm

# Set working directory
WORKDIR /app

# Install ALL dependencies (build + runtime) in a SINGLE command
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libsqlite3-dev \
    libssl3 \
    libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/*

# Copy entire project
COPY . .

# Set environment variable for x86-64 compilation
ENV RUSTTARGET=x86_64-unknown-linux-gnu

# Build the application
RUN cargo build --release

# Copy assets to the root directory where the binary will run
RUN mv target/release/xmr-contracts .

# Enable logging
ENV RUST_LOG=debug
ENV RUST_BACKTRACE=full

EXPOSE 8080

# Run the application from the root directory
CMD ["./xmr-contracts"]

