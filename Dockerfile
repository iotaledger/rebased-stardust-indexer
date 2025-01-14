# Build Stage
FROM rust:bullseye AS builder
ARG PROFILE=release
ARG GIT_REVISION
ENV GIT_REVISION=$GIT_REVISION

WORKDIR "/app"

# Install build dependencies
RUN apt-get update && apt-get install -y cmake clang libpq-dev

# Copy relevant files
COPY Cargo.toml Cargo.lock ./
COPY src src
COPY migrations migrations

# Build the application binary
RUN cargo build --profile ${PROFILE} --bin rebased-stardust-indexer

# Extract the built binary
RUN mv target/$(if [ $PROFILE = "dev" ]; then echo "debug"; else echo "release";fi)/rebased-stardust-indexer ./

# Runtime Stage
FROM debian:bullseye-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y libsqlite3-0 && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the compiled binary from the builder stage
COPY --from=builder /app/rebased-stardust-indexer /usr/local/bin

# Copy runtime-specific assets
COPY migrations /app/migrations

# Metadata for the image
ARG BUILD_DATE
ARG GIT_REVISION
LABEL build-date=$BUILD_DATE
LABEL git-revision=$GIT_REVISION

# Default command to run the application
CMD ["rebased-stardust-indexer"]
