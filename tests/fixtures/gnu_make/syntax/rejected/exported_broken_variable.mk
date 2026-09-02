# run-recipes: true
# line: 4
# rumk: Unterminated variable reference: missing ')' (GNU Make fails when 'BROKEN' is expanded)
BROKEN = $(X
export BROKEN
all: ; @echo ok
