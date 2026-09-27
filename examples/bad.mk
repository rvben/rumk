# A Makefile with problems rumk reports by default. `rumk check examples/bad.mk`
# lists each one; `rumk rule <CODE>` explains it.

BUILD_DIR := build

# MK201: command targets are not declared .PHONY
all: app test

# MK001: the recipe is indented with spaces, not a tab
app: main.c
    cc -o app main.c

# MK211: $BUILD_DIR is read by Make as $(B) followed by "UILD_DIR"
clean:
	rm -rf $BUILD_DIR app

# MK212: each recipe line runs in its own shell, so the cd is lost
# MK203: a recursive make call should use $(MAKE)
test: app
	cd tests
	make check

# MK101: the line is longer than 120 characters
CFLAGS = -Wall -Wextra -Werror -O2 -g -std=c11 -pedantic -Wno-unused-parameter -Wno-unused-variable -Wno-unused-function -march=native
