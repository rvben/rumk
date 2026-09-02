define FOO # := note
$(BROKEN
endef
verify:
	@test "$(flavor FOO)" = recursive
