# line: 3
# rumk: (GNU Make fails when 'Y' is expanded)
Y = $(BROKEN
Y := $(Y) more
all: ; @:
