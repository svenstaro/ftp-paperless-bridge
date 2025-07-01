set dotenv-load := true

build *args:
  cargo build -- {{ args }}

run *args:
  cargo run -- {{ args }}
