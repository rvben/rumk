include mk/tools.mk

BINARY := build/service
GO ?= go

.PHONY: all build clean lint test verify
all: build

build:
	$(GO) build -o $(BINARY) ./cmd/service

test:
	$(GO) test ./...

lint: tools
	$(GOLANGCI_LINT) run

clean:
	$(RM) -r build

verify:
	@:
