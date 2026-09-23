.PHONY: help install run stop test check format build standalone wasm bench browser bench-web dist charts library ranking clean

RUST_IMAGE ?= rust:1.90-bookworm
NODE_IMAGE ?= node:22-bookworm-slim
PORT ?=

DOCKER_RUN = docker run --rm --user "$$(id -u):$$(id -g)" -v "$(CURDIR)":/app
CARGO = $(DOCKER_RUN) -w /app -e CARGO_HOME=/app/.cargo-home -e CARGO_TERM_COLOR=always $(RUST_IMAGE) cargo
NPM = $(DOCKER_RUN) -w /app/web -e npm_config_cache=/app/.npm-cache -e HOME=/app $(NODE_IMAGE) npm

help:
	@echo "make run       start the app (http://localhost:5173, or PROD=1 on :8080)"
	@echo "make standalone build one HTML file that runs from disk"
	@echo "make stop      stop it"
	@echo "make test      run the Rust suite, the type-checker and the web suite"
	@echo "make check     everything CI runs: format, lint, test, build"
	@echo "make format    reformat Rust and web sources"
	@echo "make wasm      build the engine into web/wasm for a local toolchain"
	@echo "make browser   check fit, feel, touch, colour and grip in a real browser"
	@echo "make bench-web what the engine costs in the browser"
	@echo "make charts     re-read the solver screenshots into the preflop library"
	@echo "make bench      time the preflop pass over all 22,100 flops"
	@echo "make library    print the preflop library as a table of percentages"
	@echo "make ranking   regenerate the hand-strength table"
	@echo "make build     build the production image"
	@echo "make clean     remove build output and caches"

install:
	$(NPM) install

run:
	docker compose --profile $(if $(PROD),prod,dev) up --build

stop:
	docker compose --profile dev --profile prod down

test:
	$(CARGO) test --workspace
	$(NPM) run typecheck
	$(NPM) test

check:
	$(CARGO) fmt --all -- --check
	$(CARGO) clippy --workspace --all-targets -- -D warnings
	$(CARGO) test --workspace
	$(NPM) run format:check
	$(NPM) run typecheck
	$(NPM) test
	$(NPM) run build

format:
	$(CARGO) fmt --all
	$(NPM) run format

wasm:
	docker build --target wasm-artifact --output type=local,dest=web/wasm .

charts:
	python3 scripts/charts/parse_all.py
	python3 scripts/charts/gen_charts.py
	$(CARGO) fmt --all

# Does the built page fit on a screen, in every setting, without scrolling?
# A real browser, because jsdom has no layout and so cannot answer it. Not part
# of `check`: it wants a browser image and half a minute, and what it guards
# against is a slow drift rather than something a wrong line of code does
# straight away.
PUPPETEER = docker run --rm --user "$$(id -u):$$(id -g)" -v "$$(pwd)":/app -w /app \
	--entrypoint node -e NODE_PATH=/home/pptruser/node_modules -e HOME=/tmp \
	-e PUPPETEER_CACHE_DIR=/home/pptruser/.cache/puppeteer \
	ghcr.io/puppeteer/puppeteer:23.11.1

browser: dist standalone
	$(PUPPETEER) scripts/browser/fits.mjs
	$(PUPPETEER) scripts/browser/feel.mjs
	$(PUPPETEER) scripts/browser/touch.mjs
	$(PUPPETEER) scripts/browser/theme.mjs
	$(PUPPETEER) scripts/browser/controls.mjs
	$(PUPPETEER) scripts/browser/standalone.mjs

# What the engine costs in the browser rather than in Rust. Prints rather than
# passes or fails: it is a number to know, and a number that moves with the
# machine it is measured on.
bench-web: dist
	$(PUPPETEER) scripts/browser/bench.mjs

dist:
	$(NPM) run build

bench:
	$(CARGO) run --quiet --release -p kongzilla-core --example bench_preflop

library:
	$(CARGO) run --quiet -p kongzilla-core --example library_table

ranking:
	$(CARGO) run --release -p kongzilla-core --example gen_ranking \
	  > crates/kongzilla-core/src/ranking_table.rs
	$(CARGO) fmt --all

standalone:
	$(NPM) run build:standalone
	@echo "web/dist-standalone/index.html - open it in a browser, no server needed"

build:
	docker build --target production --tag kongzilla:local .
	@echo "kongzilla:local - serves the site; run it with /usr/local/bin/kongzilla-beacon to count visits"

clean:
	rm -rf target web/dist web/dist-standalone web/wasm web/node_modules .cargo-home .npm-cache
