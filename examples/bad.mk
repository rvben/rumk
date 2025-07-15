# Example Makefile with various issues

# Missing .PHONY declaration
clean:
    rm -rf build/

# Mixed indentation (spaces instead of tabs)
build: src/*.c
        gcc -o app $^

# Poor variable naming
foo = bar
FOO = baz

# Hardcoded path
INSTALL_DIR = /usr/local/bin

# Missing dependencies
test:
	pytest tests/

# Long line
CFLAGS = -Wall -Wextra -Werror -O2 -g -std=c11 -pedantic -Wno-unused-parameter -Wno-unused-variable -Wno-unused-function -march=native

all: build test

install: build
	cp app $(INSTALL_DIR)