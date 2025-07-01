set dotenv-load := true

build *args:
  cargo build -- {{ args }}

build-container: build
  podman build -f Containerfile -t ftp-paperless-bridge:local target/debug

run *args:
  cargo run -- {{ args }}
