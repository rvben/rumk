# make: >=4.3
# The same two names as modifier_named_variables.mk, with the operators the
# other way round, so each spelling stands on an assertion of its own.
export := 3
override = 4
verify:
	@test "$(export)" = "3"
	@test "$(override)" = "4"
