include mk/frontend.mk
include mk/backend.mk

.PHONY: all clean test verify
all: frontend backend

test: frontend-test backend-test

clean:
	$(RM) -r build

verify:
	@:
