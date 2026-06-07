default:
    @just --list

init:
    bun install

build-frontend:
    bun run build

build-backend:
    cargo build --release --manifest-path backend/Cargo.toml

build: build-frontend build-backend

run-frontend-dev *args:
    bun run dev --host {{ args }}

run-backend-dev *args:
    cargo run --manifest-path backend/Cargo.toml -- --dev -vv {{ args }}

dev:
    #!/usr/bin/env sh
    cargo run --manifest-path backend/Cargo.toml -- --dev -vv &
    BACKEND_PID=$!
    trap "kill $BACKEND_PID" EXIT INT TERM
    bun run dev --host
    kill $BACKEND_PID 2>/dev/null || true

run-backend *args:
    cargo run --release --manifest-path backend/Cargo.toml -- {{ args }}

test *args:
    cargo test --manifest-path backend/Cargo.toml {{ args }}

format:
    treefmt
