verify: Y = $(BROKEN
dep: Y = fine
verify: dep
	@:
dep:
	@test "$(Y)" = "fine"
