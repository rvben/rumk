# make: >=4.3
# GNU Make 4.3 and later skip a byte order mark before the first line, so
# this file reads as if the mark were not there. Earlier versions read it as
# part of the first word and stop with a missing separator.
.PHONY: verify
verify:
	@echo ok
