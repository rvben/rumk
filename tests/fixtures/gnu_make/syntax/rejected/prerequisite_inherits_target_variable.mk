# line: 4
# rumk: (GNU Make fails when 'Y' is expanded)
all: dep
all: Y = $(BROKEN
dep: ; @echo $(Y)
