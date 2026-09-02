# line: 7
# rumk: Unterminated variable reference: missing ')'
X := 1
ifeq (a,b)
X = 2
endif
X += $(BROKEN
all: ; @:
