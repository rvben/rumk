# make: >=4.3
# line: 8
# rumk: Missing separator: line is not a rule, an assignment, or a directive
P = >
.RECIPEPREFIX =
.RECIPEPREFIX += $(P)
verify:
>@echo ok
