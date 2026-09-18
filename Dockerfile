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

# Unprivileged nginx: listens on 8080 as uid 101 and keeps its temporary paths
# under /tmp, so the cluster can run it read-only without a shim.
FROM nginxinc/nginx-unprivileged:1.27-alpine AS production
COPY --chmod=0644 web/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=web-builder /app/web/dist /usr/share/nginx/html
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD wget -qO- http://127.0.0.1:8080/healthz >/dev/null || exit 1

# ---------------------------------------------------------------------------
# Beacon: visit statistics, as Prometheus metrics. A separate image because it
# is a separate concern with a separate lifetime - the site is static files on
# a CDN-shaped server, this is a process that holds a day's counters.
# ---------------------------------------------------------------------------
FROM rust-base AS beacon-builder
RUN cargo build --release -p kongzilla-beacon

FROM gcr.io/distroless/cc-debian12:nonroot AS beacon
COPY --from=beacon-builder /app/target/release/kongzilla-beacon /usr/local/bin/kongzilla-beacon
USER nonroot
EXPOSE 8080 9090
ENTRYPOINT ["/usr/local/bin/kongzilla-beacon"]
