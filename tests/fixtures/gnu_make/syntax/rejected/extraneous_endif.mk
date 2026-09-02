# line: 5
# rumk: 'endif' without a matching conditional
ifeq (a,a)
endif
endif
all: ; @echo ok
