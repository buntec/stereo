default:
    @just --list

init:
    bun install

build-frontend:
    bun run build

build-backend-py:
    uv build

build-backend:
    cargo build --release --manifest-path backend/Cargo.toml

build: build-frontend build-backend

run-frontend-dev *args:
    bun run dev --host {{ args }}

run-backend-dev *args:
    cargo run --manifest-path backend/Cargo.toml -- --dev -vv {{ args }}

run-backend *args:
    cargo run --release --manifest-path backend/Cargo.toml -- {{ args }}

test *args:
    uv run pytest tests/ {{ args }}

format:
    treefmt
