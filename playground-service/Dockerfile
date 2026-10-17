FROM rust:bookworm AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock build.rs ./
COPY src ./src
RUN cargo build --locked --release --bin rfort --config profile.release.lto=false --config profile.release.codegen-units=16

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends python3 gcc libc6-dev binutils \
    && rm -rf /var/lib/apt/lists/*
ARG RFORT_COMMIT=unknown
LABEL org.opencontainers.image.source="https://github.com/Beliavsky/rfortran" \
      org.opencontainers.image.revision=$RFORT_COMMIT
ENV RFORT_COMPILER=/opt/rfort/rfort PYTHONDONTWRITEBYTECODE=1 TMPDIR=/work
COPY --from=build /build/target/release/rfort /opt/rfort/rfort
COPY playground-service/protocol.py playground-service/worker.py /opt/rfort/
USER 65532:65532
WORKDIR /work
ENTRYPOINT ["python3", "/opt/rfort/worker.py"]
