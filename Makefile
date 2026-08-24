.PHONY: check fmt spacing lint test build

check: fmt spacing lint test

fmt:
	cargo fmt --all -- --check

spacing:
	./scripts/check-spacing.sh

lint:
	cargo clippy --all-targets --all-features --locked -- -D warnings

test:
	cargo test --all-targets --all-features --locked

build:
	cargo build --release --locked
