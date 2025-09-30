CARGO ?= cargo
PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin

.PHONY: build install test clean

build:
	$(CARGO) build --release

install: build
	install -Dm755 target/release/kdownload $(DESTDIR)$(BINDIR)/kdownload
	install -Dm644 LICENSE $(DESTDIR)$(PREFIX)/share/licenses/kdownload/LICENSE

test:
	$(CARGO) test

clean:
	$(CARGO) clean
