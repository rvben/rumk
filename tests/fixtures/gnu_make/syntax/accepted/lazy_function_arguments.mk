Y = $(BROKEN
A := $(if ,$(Y),ok)
B := $(or ok,$(Y))
C := $(and ,$(Y))
D := $(foreach v,,$(Y))
E := $(if x,ok,$(Y))
verify:
	@test "$(A)$(B)$(C)$(D)$(E)" = okokok
