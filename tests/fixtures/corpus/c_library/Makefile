PROJECT := demo
SRC_DIR := src
BUILD_DIR := build
SOURCES := $(addprefix $(SRC_DIR)/,main.c util.c)
OBJECTS := $(SOURCES:$(SRC_DIR)/%.c=$(BUILD_DIR)/%.o)
DEPS := $(OBJECTS:.o=.d)

CPPFLAGS += -Iinclude
CFLAGS ?= -O2

-include $(DEPS)

.PHONY: all check clean verify
all: $(BUILD_DIR)/$(PROJECT)

$(BUILD_DIR)/$(PROJECT): $(OBJECTS)
	$(CC) $(LDFLAGS) -o $@ $^

$(BUILD_DIR)/%.o: $(SRC_DIR)/%.c
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) -MMD -MP -c -o $@ $<

check verify:
	@:

clean:
	$(RM) -r $(BUILD_DIR)
