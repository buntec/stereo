default:
    @just --list

init:
    bun install

build-frontend:
    bun run build

build-backend:
    cargo build --release

build: build-frontend build-backend

install: build
  cargo install --path .

dev-port := "8055"

run-frontend-dev *args:
    STEREO_PORT={{ dev-port }} bun run dev --host {{ args }}

run-backend-dev *args:
    cargo run -- --dev -vv --port {{ dev-port }} {{ args }}

dev:
    #!/usr/bin/env sh
    cargo run -- --dev -vv --port {{ dev-port }} &
    BACKEND_PID=$!
    trap "kill $BACKEND_PID" EXIT INT TERM
    STEREO_PORT={{ dev-port }} bun run dev
    kill $BACKEND_PID 2>/dev/null || true

# Build the app and serve it over HTTPS on your tailnet too, e.g. to test on a phone: open
# http://localhost:8055 and scan the QR code from the Tailscale button
dev-tailscale: build-frontend
    cargo run -- -v --tailscale --port {{ dev-port }}

exe := if os() == "windows" { ".exe" } else { "" }

# Copy a backend build into the desktop app's bundle inputs, where Tauri expects its sidecar.
_desktop-sidecar profile:
    mkdir -p desktop/src-tauri/binaries
    cp target/{{ profile }}/stereo{{ exe }} desktop/src-tauri/binaries/stereo-$(rustc --print host-tuple){{ exe }}

# Build and run the desktop app with a debug backend. If a stereo already runs on port 8005, the
# app opens that one instead: stop it, or pick another port with STEREO_PORT, to try the bundled one
desktop-dev: build-frontend
    cargo build
    just _desktop-sidecar debug
    cd desktop && bun install && bun run dev

# Package the desktop app with a release backend, under desktop/src-tauri/target/release/bundle/
desktop-build: build
    just _desktop-sidecar release
    cd desktop && bun install
    if [ "$(uname -s)" = "Darwin" ]; then cd desktop && bun run build -- --bundles app; else cd desktop && bun run build; fi

# Check and test the desktop app's launcher
desktop-check:
    cargo build
    just _desktop-sidecar debug
    cd desktop/src-tauri && cargo clippy && cargo test

run-backend *args:
    cargo run --release -- {{ args }}

test *args:
    cargo test {{ args }}

lint:
    cargo clippy

format:
    cargo fmt
    cargo fmt --manifest-path desktop/src-tauri/Cargo.toml
    bun run prettier --write "frontend/src/**/*.{ts,tsx,css}"

bump part="patch":
    #!/usr/bin/env sh
    set -eu
    cargo set-version --bump {{ part }}
    version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
    cargo set-version --manifest-path desktop/src-tauri/Cargo.toml "$version"
