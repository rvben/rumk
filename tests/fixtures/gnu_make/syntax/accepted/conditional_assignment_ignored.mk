Y = fine
Y ?= $(BROKEN
verify:
	@test "$(Y)" = "fine"
