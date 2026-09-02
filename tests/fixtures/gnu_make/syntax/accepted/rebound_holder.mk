Y = $(BROKEN
Z = $(Y)
Z := fine
verify:
	@test "$(Z)" = "fine"
