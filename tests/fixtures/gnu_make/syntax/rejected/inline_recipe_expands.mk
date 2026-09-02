# line: 3
# rumk: (GNU Make fails when 'Y' is expanded)
Y = $(BROKEN
all: ; @echo $(Y)
