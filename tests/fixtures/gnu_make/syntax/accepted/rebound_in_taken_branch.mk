Y = $(BROKEN
ifeq (a,a)
Y = fine
endif
verify:
	@test "$(Y)" = "fine"
