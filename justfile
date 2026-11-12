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

run-backend *args:
    cargo run --release -- {{ args }}

test *args:
    cargo test {{ args }}

lint:
    cargo clippy

format:
    cargo fmt
    bun run prettier --write "frontend/src/**/*.{ts,tsx,css}"

bump part="patch":
    cargo set-version --bump {{ part }}
