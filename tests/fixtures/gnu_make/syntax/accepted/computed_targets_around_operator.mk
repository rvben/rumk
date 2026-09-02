A = x:
$(A) b := 2
verify:
	@test -z "$(b)"
