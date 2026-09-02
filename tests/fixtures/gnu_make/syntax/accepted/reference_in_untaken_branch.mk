Y = $(BROKEN
ifeq (a,b)
X := $(Y)
endif
verify:
ifeq (a,b)
	@echo $(Y)
endif
	@echo ok
