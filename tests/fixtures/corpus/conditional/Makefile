PLATFORM ?= linux

ifeq ($(PLATFORM),linux)
include mk/linux.mk
else ifeq ($(PLATFORM),windows)
include mk/windows.mk
endif

.PHONY: all verify
all: platform-build

verify:
	@:
