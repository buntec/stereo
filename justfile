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

# The WebSocket token is kept in target/, so a bookmarked URL keeps working across runs.
# Build the app and serve it over HTTPS on your tailnet, e.g. to test on a phone
dev-tailscale: build-frontend
    #!/usr/bin/env bash
    set -euo pipefail
    port={{ dev-port }} # the backend's port on 127.0.0.1, and the HTTPS port on the tailnet
    host=$(tailscale status --json | bun -e 'console.log((await Bun.stdin.json()).Self.DNSName.replace(/\.$/, ""))')
    url="https://$host:$port"
    token_file=target/dev-tailscale-token
    if [ ! -s "$token_file" ]; then
        mkdir -p target
        bun -e 'console.log(crypto.randomUUID().replaceAll("-", ""))' > "$token_file"
    fi
    export STEREO_WS_TOKEN=$(cat "$token_file")
    cargo build
    tailscale serve --bg --yes --https=$port http://127.0.0.1:$port > /dev/null
    trap "tailscale serve --https=$port off > /dev/null 2>&1 || true" EXIT
    app_url="$url/#stereo-token=$STEREO_WS_TOKEN"
    printf '\nOpen on a device in your tailnet, or scan:\n\n  %s\n\n' "$app_url"
    QR_TEXT=$app_url bun -e 'import QRCode from "qrcode"; console.log(await QRCode.toString(process.env.QR_TEXT, { type: "terminal", small: true }))'
    cargo run -- -v --host 127.0.0.1 --port $port --allowed-origin "$url"

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
