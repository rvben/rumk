# Recipe-prefixed lines with no open rule are ordinary statements.
	FOO = 1
	# a comment
	ifeq (1,1)
	BAR := 2
	endif
verify:
	@test "$(FOO)" = "1"
	@test "$(BAR)" = "2"
