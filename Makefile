prepare:
	cargo install cargo-criterion
	pip install -r requirements-dev.txt
linting:
	cargo clippy --tests --benches --features linting
build:
	maturin build --release -i 3.9
test:
	cargo nextest run --features linting
	maturin develop --release
	pytest -s --show-capture all
bench:
	cargo bench
clean:
	cargo clean
