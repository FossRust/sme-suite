set shell := ["bash", "-uc"]
default: build
build:      cargo build
fmt:        cargo fmt --all
lint:       cargo clippy --all-features -- -D warnings
dev:        cargo watch -x 'run -p server -- serve'
serve:      cargo run -p server -- serve
migrate:    cargo run -p server -- migrate up
reset-db:   cargo run -p server -- migrate reset
seed:       cargo run -p server -- seed
schema:     cargo run -p server -- print-schema
