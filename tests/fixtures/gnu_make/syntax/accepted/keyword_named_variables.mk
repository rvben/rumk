# make: >=4.3
ifdef = 1
include := 2
define = 3
vpath ?= 4
verify:
	@test "$(ifdef)$(include)$(define)$(vpath)" = "1234"
