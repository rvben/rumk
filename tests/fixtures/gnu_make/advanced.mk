# This fixture must parse on GNU Make 3.81 and newer.
SOURCES := one.c \
           two.c
OBJECTS := $(SOURCES:%.c=build/%.o)

-include local-overrides.mk

ifdef DEBUG
MODE := debug
else
MODE := release
endif

define banner
mode: $(MODE)
objects: $(OBJECTS)
endef

.PHONY: validate prepare stamp

validate: LABEL := $(subst x,x,value:with=delimiters)
validate: prepare | stamp
	@printf '%s\n' '$(call banner-for,$(MODE):checked=yes)' \
	  '$(LABEL)'

prepare stamp:
	@:

one.o two.o: %.o: %.c
	@printf '%s\n' '$< -> $@'

$(addprefix generated/,one two):
	@:
