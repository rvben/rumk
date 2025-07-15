# Example of a well-formatted Makefile

.PHONY: all clean test build install help

# Configuration
CC = gcc
CFLAGS = -Wall -Wextra -O2
INSTALL_PREFIX = /usr/local
INSTALL_DIR = $(INSTALL_PREFIX)/bin

# Main targets
all: build test

build: app

app: src/*.c
	$(CC) $(CFLAGS) -o $@ $^

test:
	pytest tests/

clean:
	rm -rf build/
	rm -f app

install: app
	install -m 755 app $(INSTALL_DIR)

help:
	@echo "Available targets:"
	@echo "  all     - Build and test"
	@echo "  build   - Build the application"
	@echo "  test    - Run tests"
	@echo "  clean   - Remove build artifacts"
	@echo "  install - Install the application"