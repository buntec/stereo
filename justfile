default:
    @just --list

init:
    bun install

build-frontend:
    bun run build

build-backend:
    cargo build --release

build: build-frontend build-backend

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
    STEREO_PORT={{ dev-port }} bun run dev --host
    kill $BACKEND_PID 2>/dev/null || true

run-backend *args:
    cargo run --release -- {{ args }}

test *args:
    cargo test {{ args }}

format:
    cargo fmt
    bun run prettier --write "frontend/src/**/*.{ts,tsx,css}"

bump part="patch":
    cargo set-version --bump {{ part }}
