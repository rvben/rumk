$(info an expression line)
X := $(A (b)
Y := $(info)
Z := ${if $(X),(x),y}
verify:
	@test "$(X)" = ""
	@test "$(Z)" = "y"
