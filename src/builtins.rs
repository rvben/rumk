//! The variables GNU Make defines itself, before any makefile or the
//! environment names them.

/// Variables GNU Make defines before it reads the first makefile, as
/// `make -p -f /dev/null` lists them for GNU Make 4.4: the programs and
/// flags of the built-in rules, and the variables that describe the
/// running make. Running make with `-R` leaves the programs and flags
/// undefined.
pub(crate) const DEFINED_AT_STARTUP: &[&str] = &[
    ".DEFAULT_GOAL",
    ".FEATURES",
    ".INCLUDE_DIRS",
    ".LIBPATTERNS",
    ".LOADED",
    ".RECIPEPREFIX",
    ".SHELLFLAGS",
    ".VARIABLES",
    "AR",
    "ARFLAGS",
    "AS",
    "CC",
    "CHECKOUT,v",
    "CO",
    "COFLAGS",
    "COMPILE.C",
    "COMPILE.F",
    "COMPILE.S",
    "COMPILE.c",
    "COMPILE.cc",
    "COMPILE.cpp",
    "COMPILE.def",
    "COMPILE.f",
    "COMPILE.m",
    "COMPILE.mod",
    "COMPILE.p",
    "COMPILE.r",
    "COMPILE.s",
    "CPP",
    "CTANGLE",
    "CURDIR",
    "CWEAVE",
    "CXX",
    "F77",
    "F77FLAGS",
    "FC",
    "GET",
    "LD",
    "LEX",
    "LEX.l",
    "LEX.m",
    "LINK.C",
    "LINK.F",
    "LINK.S",
    "LINK.c",
    "LINK.cc",
    "LINK.cpp",
    "LINK.f",
    "LINK.m",
    "LINK.o",
    "LINK.p",
    "LINK.r",
    "LINK.s",
    "LINT",
    "LINT.c",
    "M2C",
    "MAKE",
    "MAKEFILES",
    "MAKEFILE_LIST",
    "MAKEFLAGS",
    "MAKEINFO",
    "MAKELEVEL",
    "MAKE_COMMAND",
    "MAKE_HOST",
    "MAKE_VERSION",
    "MFLAGS",
    "OBJC",
    "OUTPUT_OPTION",
    "PC",
    "PREPROCESS.F",
    "PREPROCESS.S",
    "PREPROCESS.r",
    "RM",
    "SHELL",
    "SUFFIXES",
    "TANGLE",
    "TEX",
    "TEXI2DVI",
    "WEAVE",
    "YACC",
    "YACC.m",
    "YACC.y",
];

/// Variables GNU Make defines only in some runs: when goals are named on
/// the command line, when it restarts after remaking a makefile, or when
/// its output is a terminal.
pub(crate) const DEFINED_IN_SOME_RUNS: &[&str] = &[
    "MAKECMDGOALS",
    "MAKE_RESTARTS",
    "MAKE_TERMERR",
    "MAKE_TERMOUT",
];

/// Whether GNU Make itself defines `name` before it reads a makefile.
pub(crate) fn is_defined_at_startup(name: &str) -> bool {
    DEFINED_AT_STARTUP.contains(&name)
}

/// Whether GNU Make may define `name` without the makefile assigning it.
pub(crate) fn is_defined_by_make(name: &str) -> bool {
    is_defined_at_startup(name) || DEFINED_IN_SOME_RUNS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_sorted_and_disjoint() {
        for table in [DEFINED_AT_STARTUP, DEFINED_IN_SOME_RUNS] {
            assert!(table.windows(2).all(|pair| pair[0] < pair[1]));
        }
        assert!(!DEFINED_IN_SOME_RUNS
            .iter()
            .any(|name| is_defined_at_startup(name)));
    }
}
