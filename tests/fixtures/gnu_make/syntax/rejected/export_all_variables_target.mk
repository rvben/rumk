# run-recipes: true
# line: 4
# rumk: Unterminated variable reference: missing ')' (GNU Make fails when 'BROKEN' is expanded)
BROKEN = $(X
.EXPORT_ALL_VARIABLES:
unexport
all: ; @echo ok
