all: build

build:
	cargo build --release

install: build
	sudo install -Dm755 target/release/dothub /usr/bin/
