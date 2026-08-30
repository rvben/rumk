# Keep this fixture compatible with GNU Make 3.81 and newer.
DIR := mk
NAMES = one.src two.src
FILES = $(addprefix $(DIR)/,$(NAMES:.src=.mk))
MODE ?= release
SOURCES = src/one.c src/two.cc README
OBJECTS = $(SOURCES:.c=.o)
WINDOW = $(wordlist 2,3,one two three four)
PATHS = $(dir src/main.c README)
BASENAMES = $(notdir src/main.c README trailing/)
LOGICAL = $(if $(MODE),$(or ,selected),$(shell false))

ifeq ($(strip $(MODE)),release)
include $(if $(MODE),$(FILES),$(shell false))
endif

TARGET := all
DEPS = one two
$(TARGET): $(DEPS)

.PHONY: verify
verify:
	@test "$(FILES)" = "mk/one.mk mk/two.mk"
	@test "$(MODE)" = "release"
	@test "$(OBJECTS)" = "src/one.o src/two.cc README"
	@test "$(WINDOW)" = "two three"
	@test "$(PATHS)" = "src/ ./"
	@test "$(BASENAMES)" = "main.c README "
	@test "$(LOGICAL)" = "selected"
