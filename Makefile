.PHONY: build test examples lint run clean

build:            ## Build an optimized binary at target/release/fleet-sim
	cargo build --release

test:             ## Run the full test suite (unit, spec examples, errors, fuzz, CLI)
	cargo test --release

examples: build   ## Diff each examples/*.jsonl against its golden output
	./scripts/check_examples.sh

lint:             ## Formatting and clippy, warnings as errors
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings

run: build        ## Interactive session: type commands, one per line
	./target/release/fleet-sim

clean:
	cargo clean
