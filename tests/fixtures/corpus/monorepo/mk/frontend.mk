FRONTEND_OUTPUT := build/frontend/app.js

.PHONY: frontend frontend-test
frontend: $(FRONTEND_OUTPUT)

$(FRONTEND_OUTPUT):
	@mkdir -p $(@D)
	@printf '%s\n' bundle >$@

frontend-test: frontend
	@printf '%s\n' frontend-tests
