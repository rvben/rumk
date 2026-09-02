# make: >=4.3
X := $(subst #,-,a#b)
verify:
	@test "$(X)" = "a-b"
