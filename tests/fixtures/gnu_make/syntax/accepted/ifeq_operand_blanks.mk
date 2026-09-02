ifeq (a,a )
this line is never read
endif
ifeq ( a,a)
this line is never read either
endif
verify: ; @echo ok
