# line: 3
# rumk: (GNU Make fails when 'Y' is expanded)
Y = $(BROKEN
ifdef FIXED
Y = ok
endif
all: $(Y)
