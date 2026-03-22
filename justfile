install:
    cargo build --release --locked
    cp -f target/release/claude-dock ~/.local/bin/claude-dock

test:
    cargo test --locked

lint:
    cargo clippy --locked -- -D warnings

fmt:
    cargo fmt --check

check: lint fmt test
