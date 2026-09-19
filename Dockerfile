# syntax=docker/dockerfile:1

# ---------------------------------------------------------------------------
# Rust: the analysis engine, and the same engine compiled to WebAssembly.
# ---------------------------------------------------------------------------
FROM rust:1.90-bookworm AS rust-base
ENV CARGO_TERM_COLOR=always
RUN rustup component add rustfmt clippy \
 && rustup target add wasm32-unknown-unknown \
 && cargo install wasm-bindgen-cli --version 0.2.128 --locked
WORKDIR /app
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates

FROM rust-base AS wasm-builder
RUN cargo build --release --target wasm32-unknown-unknown -p kongzilla-wasm \
 && wasm-bindgen --target web --out-dir /wasm \
      target/wasm32-unknown-unknown/release/kongzilla_wasm.wasm

# Exportable artefact: `make wasm` writes this stage straight into web/wasm.
FROM scratch AS wasm-artifact
COPY --from=wasm-builder /wasm /

# ---------------------------------------------------------------------------
# Web application.
# ---------------------------------------------------------------------------
FROM node:22-bookworm-slim AS web-deps
WORKDIR /app/web
COPY web/package.json web/package-lock.json* ./
RUN npm install --no-audit --no-fund

FROM web-deps AS dev
WORKDIR /app/web
COPY web ./
COPY --from=wasm-builder /wasm ./wasm
EXPOSE 5173
CMD ["npm", "run", "dev", "--", "--host", "0.0.0.0", "--port", "5173"]

FROM web-deps AS web-builder
WORKDIR /app/web
COPY web ./
COPY --from=wasm-builder /wasm ./wasm
RUN npm run build

# ---------------------------------------------------------------------------
# Beacon: visit statistics, as Prometheus metrics.
#
# Built against musl and linked static, so the one binary runs anywhere - in
# particular inside the Alpine image below, which is where it goes. The target
# is derived from the build machine rather than written down, so this builds on
# an ARM laptop as well as on the amd64 runner.
# ---------------------------------------------------------------------------
FROM rust-base AS beacon-builder
RUN apt-get update \
 && apt-get install -y --no-install-recommends musl-tools \
 && rm -rf /var/lib/apt/lists/*
RUN TARGET="$(uname -m)-unknown-linux-musl" \
 && rustup target add "$TARGET" \
 && cargo build --release --target "$TARGET" -p kongzilla-beacon \
 && cp "target/$TARGET/release/kongzilla-beacon" /kongzilla-beacon

# ---------------------------------------------------------------------------
# The one image, with two things to be.
#
# It used to be two: a server for the static files and a process that counts
# visits. They are still two deployments - different lifetimes, different
# scaling, and only one of them may hold the day's counters - but two images
# meant two builds, two pushes, two tags to keep level with each other, and a
# release where they could drift a version apart. One image, and which of the
# two it is is a matter of the command it is started with: nothing, and it
# serves the site; the beacon binary, and it counts.
#
# Unprivileged nginx: listens on 8080 as uid 101 and keeps its temporary paths
# under /tmp, so the cluster can run it read-only without a shim. The beacon
# wants nothing else: static binary, no writes, same user.
# ---------------------------------------------------------------------------
FROM nginxinc/nginx-unprivileged:1.27-alpine AS production
COPY --chmod=0644 web/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=web-builder /app/web/dist /usr/share/nginx/html
COPY --from=beacon-builder --chmod=0755 /kongzilla-beacon /usr/local/bin/kongzilla-beacon
# 8080 is the site, or the beacon's public endpoint; 9090 is the beacon's
# metrics, which only the cluster ever reaches.
EXPOSE 8080 9090
# Both roles answer this, on the same port, which is the whole reason one
# healthcheck can cover an image that is two things.
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD wget -qO- http://127.0.0.1:8080/healthz >/dev/null || exit 1
