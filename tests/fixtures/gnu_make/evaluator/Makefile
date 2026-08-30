# Keep this fixture compatible with GNU Make 3.81 and newer.
DIR := mk
NAMES = one two
FILES = $(addsuffix .mk,$(addprefix $(DIR)/,$(NAMES)))
MODE ?= release

ifeq ($(strip $(MODE)),release)
include $(FILES)
endif

TARGET := all
DEPS = one two
$(TARGET): $(DEPS)

.PHONY: verify
verify:
	@test "$(FILES)" = "mk/one.mk mk/two.mk"
	@test "$(MODE)" = "release"
