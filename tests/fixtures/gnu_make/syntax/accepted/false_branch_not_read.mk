ifeq (a,b)
this is not a statement
	$(unclosed
X := $(FOO
= 1
all: = 1
else ifneq (a,a)
another non-statement
else
OK := 1
endif
verify:
	@test "$(OK)" = "1"
