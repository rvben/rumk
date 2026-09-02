override Y = $(BROKEN
verify: Y = fine
verify:
	@test "$(Y)" = "fine"
