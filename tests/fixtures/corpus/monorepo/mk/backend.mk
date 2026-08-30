BACKEND_OUTPUT := build/backend/service

.PHONY: backend backend-test
backend: $(BACKEND_OUTPUT)

$(BACKEND_OUTPUT):
	@mkdir -p $(@D)
	@printf '%s\n' binary >$@

backend-test: backend
	@printf '%s\n' backend-tests
