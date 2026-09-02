ifeq (a,a)
OK := 1
else ifeq ($(BROKEN,x
endif
verify:
	@test "$(OK)" = "1"
