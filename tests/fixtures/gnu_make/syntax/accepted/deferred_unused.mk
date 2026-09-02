# A recursively expanded variable is only expanded when it is used.
Y = $(FOO
Y ?= $(BAR
define D
$(BAZ
endef
verify:
	@true
