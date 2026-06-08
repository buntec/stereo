default:
    @just --list

init:
    bun install

build-frontend:
    bun run build

build-backend:
    cargo build --release

build: build-frontend build-backend

run-frontend-dev *args:
    bun run dev --host {{ args }}

run-backend-dev *args:
    cargo run -- --dev -vv {{ args }}

dev:
    #!/usr/bin/env sh
    cargo run -- --dev -vv &
    BACKEND_PID=$!
    trap "kill $BACKEND_PID" EXIT INT TERM
    bun run dev --host
    kill $BACKEND_PID 2>/dev/null || true

run-backend *args:
    cargo run --release -- {{ args }}

test *args:
    cargo test {{ args }}

format:
    cargo fmt
    bun run prettier --write "frontend/src/**/*.{ts,tsx,css}"
