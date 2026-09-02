# line: 3
# rumk: (GNU Make fails when 'Y' is expanded)
override Y = $(BROKEN
Y = ok
all: $(Y)
