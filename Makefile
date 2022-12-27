all: build

build:
	cargo build --release

install: build
	install -Dm755 target/release/dothub /usr/bin/
