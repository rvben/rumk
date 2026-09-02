define X
override define = 1
endef
verify:
	@test "$(X)" = "override define = 1"
