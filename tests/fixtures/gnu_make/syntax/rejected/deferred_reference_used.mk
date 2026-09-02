# line: 3
# rumk: Unterminated variable reference: missing ')' (GNU Make fails when 'Y' is expanded)
Y = $(FOO
all: $(Y)
