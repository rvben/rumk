# line: 8
# rumk-line: 4
# rumk: Missing 'endif' for 'ifeq (a,b)'
ifeq (a,b)
define X
endif
all: ; @echo ok
