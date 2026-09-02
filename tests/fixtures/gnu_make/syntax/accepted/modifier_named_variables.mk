# make: >=4.3
export = 1
override := 2
verify:
	@test "$(export)" = "1"
	@test "$(override)" = "2"
