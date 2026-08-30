GOLANGCI_LINT ?= golangci-lint

.PHONY: tools
tools:
	@command -v $(GOLANGCI_LINT) >/dev/null
