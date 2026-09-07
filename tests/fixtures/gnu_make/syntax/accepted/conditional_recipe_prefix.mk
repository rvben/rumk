# make: >=4.3
# Make reads line 5 as a recipe only where X is defined, and expands a broken
# reference only in a recipe. Without X the line is an assignment nothing
# expands, and Make accepts the file.
ifdef X
.RECIPEPREFIX := >
endif
verify:;@true
>FOO = $(
