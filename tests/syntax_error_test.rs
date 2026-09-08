use rumk::diagnostic::Severity;
use rumk::logical::Reach;
use rumk::parser::{parse, AssignmentOperator, SeparatorHint, SyntaxErrorKind};
use rumk::rules::syntax::InvalidSyntax;
use rumk::rules::Rule;

fn errors(source: &str) -> Vec<(SyntaxErrorKind, usize, usize)> {
    parse(source)
        .syntax_errors
        .into_iter()
        .map(|error| (error.kind, error.line, error.column))
        .collect()
}

fn missing_separator(hint: Option<SeparatorHint>) -> SyntaxErrorKind {
    SyntaxErrorKind::MissingSeparator { hint }
}

fn unterminated(
    closing: char,
    function: Option<&str>,
    deferred_in: Option<&str>,
) -> SyntaxErrorKind {
    SyntaxErrorKind::UnterminatedReference {
        closing,
        function: function.map(str::to_string),
        deferred_in: deferred_in.map(str::to_string),
    }
}

#[test]
fn a_line_that_is_not_a_statement_is_a_missing_separator() {
    assert_eq!(
        errors("FOO := 1\nthis line is not a statement\n"),
        [(missing_separator(None), 2, 1)]
    );
}

#[test]
fn missing_separator_hints_follow_gnu_make() {
    assert_eq!(
        errors("        echo hello\n"),
        [(
            missing_separator(Some(SeparatorHint::SpacesInsteadOfTab)),
            1,
            1
        )]
    );
    assert_eq!(
        errors(".RECIPEPREFIX := >\n        echo hello\n"),
        [(missing_separator(None), 2, 1)]
    );
    assert_eq!(
        errors("ifeq(a,b)\nendif\n"),
        [(
            missing_separator(Some(SeparatorHint::ConditionalWithoutSpace)),
            1,
            1
        )]
    );
}

#[test]
fn a_line_that_may_expand_to_a_statement_is_accepted() {
    assert!(errors("$(info reading)\n$(eval all:)\n$(call macro)\nFOO$(BAR) baz\n").is_empty());
}

#[test]
fn a_prefixed_line_before_any_rule_is_a_recipe_before_the_first_target() {
    assert_eq!(
        errors("\techo hello\n"),
        [(SyntaxErrorKind::RecipeBeforeTarget, 1, 1)]
    );
    assert_eq!(
        errors("\tall: dep\n"),
        [(SyntaxErrorKind::RecipeBeforeTarget, 1, 1)]
    );
    assert_eq!(
        errors("\t\\"),
        [(SyntaxErrorKind::RecipeBeforeTarget, 1, 1)]
    );
    assert!(errors("\t\\\n").is_empty());
}

#[test]
fn prefixed_statements_are_read_as_statements_when_no_rule_is_open() {
    let makefile = parse("\tFOO = 1\n\t# comment\n\tifeq (1,1)\n\tBAR := 2\n\tendif\n");

    assert!(makefile.syntax_errors.is_empty());
    assert_eq!(makefile.variables["FOO"].value, "1");
    assert_eq!(makefile.variables["BAR"].value, "2");
    assert_eq!(makefile.conditionals.len(), 2);
    assert!(makefile.rules.is_empty());
}

#[test]
fn statements_close_the_open_rule() {
    let closers = [
        "FOO = 1",
        "export FOO",
        "undefine FOO",
        "vpath %.c src",
        "include other.mk",
        "$(info an expression)",
        "define D\nendef",
    ];
    for closer in closers {
        let source = format!("all:\n\t@true\n{closer}\n\t@true\n");
        let makefile = parse(&source);
        let orphan_line = 4 + closer.matches('\n').count();

        assert_eq!(
            errors(&source),
            [(SyntaxErrorKind::RecipeBeforeTarget, orphan_line, 1)],
            "{closer:?}"
        );
        assert_eq!(makefile.rules[0].recipes.len(), 1, "{closer:?}");
    }
}

#[test]
fn comments_blank_lines_and_conditionals_keep_the_rule_open() {
    let keepers = ["", "# comment", "ifeq (1,1)\nelse\nendif", "ifdef X\nendif"];
    for keeper in keepers {
        let source = format!("all:\n\t@true\n{keeper}\n\t@true\n");
        let makefile = parse(&source);

        assert!(makefile.syntax_errors.is_empty(), "{keeper:?}");
        assert_eq!(makefile.rules[0].recipes.len(), 2, "{keeper:?}");
    }
}

#[test]
fn a_target_specific_assignment_does_not_open_a_rule() {
    let source = "all: FOO = 1\n\t@true\n";
    let makefile = parse(source);

    assert_eq!(
        errors(source),
        [(SyntaxErrorKind::RecipeBeforeTarget, 2, 1)]
    );
    assert!(makefile.rules[0].target_assignment.is_some());
    assert!(makefile.rules[0].recipes.is_empty());
}

#[test]
fn unterminated_references_are_located_and_named() {
    assert_eq!(
        errors("X := $(FOO\n"),
        [(unterminated(')', None, None), 1, 6)]
    );
    assert_eq!(
        errors("X := ${info hi\n"),
        [(unterminated('}', Some("info"), None), 1, 6)]
    );
    assert_eq!(
        errors("X := $(if $(A,b)\n"),
        [(unterminated(')', Some("if"), None), 1, 6)]
    );
    assert_eq!(
        errors("$(FOO := 1\n"),
        [(unterminated(')', None, None), 1, 1)]
    );
    assert_eq!(
        errors("all: $(FOO\n"),
        [(unterminated(')', None, None), 1, 6)]
    );
    assert_eq!(
        errors("include $(FOO\n"),
        [(unterminated(')', None, None), 1, 9)]
    );
    assert_eq!(
        errors("ifdef $(FOO\nendif\n"),
        [(unterminated(')', None, None), 1, 7)]
    );
    assert_eq!(
        errors("all:\n\techo $(FOO\n"),
        [(unterminated(')', None, None), 2, 7)]
    );
    assert_eq!(
        errors("all: ; echo $(FOO\n"),
        [(unterminated(')', None, None), 1, 13)]
    );
    assert_eq!(
        errors("X := one \\\n  $(FOO\n"),
        [(unterminated(')', None, None), 2, 3)]
    );
}

#[test]
fn deferred_references_record_the_variable_that_expands_them() {
    assert_eq!(
        errors("Y = $(FOO\n"),
        [(unterminated(')', None, Some("Y")), 1, 5)]
    );
    assert_eq!(
        errors("Y ?= $(FOO\n"),
        [(unterminated(')', None, Some("Y")), 1, 6)]
    );
    assert_eq!(
        errors("Y += $(FOO\n"),
        [(unterminated(')', None, Some("Y")), 1, 6)]
    );
    assert_eq!(
        errors("Y = 1\nY += $(FOO\n"),
        [(unterminated(')', None, Some("Y")), 2, 6)]
    );
    assert_eq!(
        errors("Y := 1\nY += $(FOO\n"),
        [(unterminated(')', None, None), 2, 6)]
    );
    assert_eq!(
        errors("define D\n$(FOO\nendef\n"),
        [(unterminated(')', None, Some("D")), 2, 1)]
    );
    assert_eq!(
        errors("define D :=\n$(FOO\nendef\n"),
        [(unterminated(')', None, None), 2, 1)]
    );
    assert_eq!(
        errors("all: Y = $(FOO\n"),
        [(unterminated(')', None, Some("Y")), 1, 10)]
    );
}

#[test]
fn appending_expands_at_once_only_to_a_certainly_simple_variable() {
    let deferred = [
        "X != echo hi\nX += $(FOO\n",
        "X :::= a\nX += $(FOO\n",
        "X ?= a\nX += $(FOO\n",
        "X := 1\nifdef Q\nX = 2\nendif\nX += $(FOO\n",
        "X := 1\nall: X += $(FOO\n",
    ];
    for source in deferred {
        let last = source.lines().count();
        let column = source.lines().last().unwrap().find("$(").unwrap() + 1;
        assert_eq!(
            errors(source),
            [(unterminated(')', None, Some("X")), last, column)],
            "{source:?}"
        );
    }

    let immediate = [
        "X := 1\nifeq (a,b)\nX = 2\nendif\nX += $(FOO\n",
        "X := 1\nifeq (a,a)\nX := 2\nendif\nX += $(FOO\n",
        "X ::= 1\nX += $(FOO\n",
    ];
    for source in immediate {
        let last = source.lines().count();
        assert_eq!(
            errors(source),
            [(unterminated(')', None, None), last, 6)],
            "{source:?}"
        );
    }
}

#[test]
fn assignments_record_whether_gnu_make_reads_them() {
    let makefile =
        parse("A := 1\nifeq (a,b)\nB := 2\nelse\nC := 3\nendif\nifdef Q\nD := 4\nendif\n");
    let reaches: Vec<_> = makefile
        .assignments
        .iter()
        .map(|variable| (variable.name.as_str(), variable.reach))
        .collect();

    assert_eq!(
        reaches,
        [
            ("A", Reach::Always),
            ("B", Reach::Never),
            ("C", Reach::Always),
            ("D", Reach::Conditional),
        ]
    );
}

#[test]
fn empty_variable_names_are_reported() {
    assert_eq!(
        errors("= 1\n"),
        [(SyntaxErrorKind::EmptyVariableName, 1, 1)]
    );
    assert_eq!(
        errors("  := 1\n"),
        [(SyntaxErrorKind::EmptyVariableName, 1, 3)]
    );
    assert_eq!(
        errors("all: = 1\n"),
        [(SyntaxErrorKind::EmptyVariableName, 1, 1)]
    );
    assert_eq!(
        errors("define\nendef\n"),
        [(SyntaxErrorKind::EmptyVariableName, 1, 1)]
    );
    assert!(errors("export = 1\noverride := 2\nprivate unexport += 3\n").is_empty());
}

#[test]
fn a_comment_ends_the_line_before_the_separator_is_looked_for() {
    assert_eq!(
        errors("garbage # x: y\n"),
        [(missing_separator(None), 1, 1)]
    );
    assert_eq!(errors("FOO # := 1\n"), [(missing_separator(None), 1, 1)]);
    assert_eq!(
        errors("plain # $(expression)\n"),
        [(missing_separator(None), 1, 1)]
    );
    assert!(errors("$(info x) # y: z\n").is_empty());
}

#[test]
fn load_directives_are_statements() {
    assert!(errors("load ../plugin.so\n-load plugin.so(init)\n").is_empty());
}

#[test]
fn an_assignment_is_found_before_any_keyword() {
    assert!(
        errors("ifdef = 1\ninclude := 2\ndefine = 3\nvpath ?= 4\nexport load = 5\n").is_empty()
    );
}

#[test]
fn a_name_ends_at_whitespace_outside_a_reference() {
    assert_eq!(errors("two words = 1\n"), [(missing_separator(None), 1, 1)]);
    assert_eq!(
        errors("two words ?= 1\n"),
        [(missing_separator(None), 1, 1)]
    );
    assert_eq!(
        errors("two words := 1\n"),
        [(SyntaxErrorKind::EmptyVariableName, 1, 1)]
    );
    assert!(errors("$(A) b = 1\n$(A) b := 2\n$(a b) = 3\nall: a b = 4\n").is_empty());
}

#[test]
fn define_blocks_must_be_balanced() {
    assert_eq!(
        errors("define BODY\necho hi\n"),
        [(
            SyntaxErrorKind::UnterminatedDefine {
                name: "BODY".into()
            },
            1,
            1
        )]
    );
    assert_eq!(
        errors("endef\n"),
        [(SyntaxErrorKind::UnexpectedEndef, 1, 1)]
    );
    assert!(errors("define OUTER\ndefine INNER\nendef\nendef\n").is_empty());
}

#[test]
fn a_define_body_nests_on_the_word_define_alone() {
    assert_eq!(
        errors("define OUTER\ndefine = 1\nendef\n"),
        [(
            SyntaxErrorKind::UnterminatedDefine {
                name: "OUTER".into()
            },
            1,
            1
        )]
    );
    assert!(errors("define OUTER\n define INNER\nendef\nendef\n").is_empty());
    assert!(errors("define OUTER\n\tendef\nendef\n").is_empty());
    assert!(errors("define OUTER\noverride define = 1\nendef\n").is_empty());
    assert!(errors("define OUTER\ndefine=1\nendef\n").is_empty());
}

#[test]
fn errors_are_ordered_by_position() {
    assert_eq!(
        errors("define D\n$(A\n"),
        [
            (
                SyntaxErrorKind::UnterminatedDefine { name: "D".into() },
                1,
                1
            ),
            (unterminated(')', None, Some("D")), 2, 1),
        ]
    );
}

#[test]
fn branches_gnu_make_never_reads_are_not_checked() {
    assert!(errors(
        "ifeq (a,b)\nnot a statement\n\t$(unclosed\nX := $(FOO\n= 1\nall: = 1\nendif\n"
    )
    .is_empty());
    assert!(errors("ifneq (a,a)\nnot a statement\nendif\n").is_empty());
    assert_eq!(
        errors("ifeq (a,b)\none\nelse ifeq (b,b)\ntwo\nelse\nthree\nendif\n"),
        [(missing_separator(None), 4, 1)]
    );
    assert_eq!(
        errors("ifeq (a,a)\none\nelse\ntwo\nendif\n"),
        [(missing_separator(None), 2, 1)]
    );
    assert_eq!(
        errors("ifdef X\none\nendif\n"),
        [(missing_separator(None), 2, 1)]
    );
    assert_eq!(
        errors("ifeq ($(X),y)\none\nendif\n"),
        [(missing_separator(None), 2, 1)]
    );
}

#[test]
fn an_else_if_after_a_taken_branch_is_skipped_unread() {
    assert!(errors("ifeq (a,a)\nOK := 1\nelse ifeq ($(BROKEN,x\nendif\n").is_empty());
    assert!(errors("ifeq (a,a)\nelse ifdef $(BROKEN\nelse ifeq ($(BROKEN,x\nendif\n").is_empty());
    assert!(errors("ifeq (a,b)\nifeq (a,b)\nelse ifeq ($(BROKEN,x\nendif\nendif\n").is_empty());
    assert_eq!(
        errors("ifeq (a,b)\nelse ifeq ($(BROKEN,x\nendif\n"),
        [(unterminated(')', None, None), 2, 12)]
    );
    assert_eq!(
        errors("ifdef X\nelse ifeq ($(BROKEN,x\nendif\n"),
        [(unterminated(')', None, None), 2, 12)]
    );
}

#[test]
fn a_branch_gnu_make_never_reads_neither_opens_nor_closes_a_rule() {
    assert_eq!(
        errors("ifeq (a,b)\nfoo:\nendif\n\t@echo hi\n"),
        [(SyntaxErrorKind::RecipeBeforeTarget, 4, 1)]
    );
    assert!(errors("all:\nifeq (a,b)\nX = 1\nendif\n\t@echo hi\n").is_empty());
    assert!(errors("all:\nifeq (a,b)\n\t@echo skipped\nendif\n\t@echo hi\n").is_empty());
}

#[test]
fn a_comment_is_stripped_before_the_condition_is_read() {
    assert!(errors("ifeq (a,b) # never\nX := $(BROKEN\nendif\n").is_empty());
    assert_eq!(
        errors("ifeq (a,a) # always\nX := $(BROKEN\nendif\n"),
        [(unterminated(')', None, None), 2, 6)]
    );

    let makefile = parse("ifeq (a,b) # c\nB := 2\nendif\n");
    assert_eq!(makefile.assignments[0].reach, Reach::Never);
}

#[test]
fn an_escaped_or_trailing_dollar_is_not_a_reference() {
    assert_eq!(
        errors("not a $$ statement\n"),
        [(missing_separator(None), 1, 1)]
    );
    assert_eq!(errors("not a $\n"), [(missing_separator(None), 1, 1)]);
    assert!(errors("not a $@ statement\n").is_empty());
    assert!(errors("not a $ statement\n").is_empty());
}

#[test]
fn a_semicolon_inside_a_rule_comment_is_not_an_inline_recipe() {
    let makefile = parse("all: dep # first; second\n\t@true\n");

    assert!(makefile.syntax_errors.is_empty());
    assert_eq!(makefile.rules[0].prerequisites, ["dep"]);
    assert_eq!(makefile.rules[0].recipes.len(), 1);
    assert!(!makefile.rules[0].recipes[0].inline);

    assert_eq!(
        errors("all: dep ; echo # $(FOO\n"),
        [(unterminated(')', None, None), 1, 19)]
    );
}

#[test]
fn the_recipe_prefix_applies_to_later_lines() {
    let source = ".RECIPEPREFIX := >\nall:\n> @true\n\t@true\n";
    let makefile = parse(source);

    assert_eq!(errors(source), [(missing_separator(None), 4, 1)]);
    assert_eq!(makefile.rules[0].recipes.len(), 1);
    assert_eq!(makefile.rules[0].recipes[0].command, "true");
    assert!(makefile.rules[0].recipes[0].silent);
}

#[test]
fn mk006_reports_every_syntax_error_as_an_error() {
    let source = "not a statement\n= 1\n\techo\n";
    let diagnostics = InvalidSyntax.check(&parse(source), source);
    let reported: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.line,
                diagnostic.column,
                diagnostic.message.as_str(),
            )
        })
        .collect();

    assert_eq!(
        reported,
        [
            (
                1,
                1,
                "Missing separator: line is not a rule, an assignment, or a directive"
            ),
            (2, 1, "Empty variable name"),
            (3, 1, "Recipe commences before first target"),
        ]
    );
    assert!(diagnostics.iter().all(|diagnostic| {
        diagnostic.rule_id == "MK006"
            && diagnostic.severity == Severity::Error
            && !diagnostic.fixable
    }));
    assert!(!InvalidSyntax.fixable());
}

fn mk006_lines(source: &str) -> Vec<usize> {
    InvalidSyntax
        .check(&parse(source), source)
        .iter()
        .map(|diagnostic| diagnostic.line)
        .collect()
}

#[test]
fn mk006_reports_a_deferred_reference_when_gnu_make_expands_that_value() {
    let expanded = [
        ("Y = $(FOO\nall: $(Y)\n", 1),
        ("Y = $(FOO\nall:\n\techo $(Y)\n", 1),
        ("Y = $(FOO\nall: ; @echo $(Y)\n", 1),
        ("Y = $(FOO\nX := $(Y)\n", 1),
        ("Y = $(FOO\nY := $(Y) more\n", 1),
        ("Y = $(FOO\nifeq ($(Y),x)\nendif\n", 1),
        ("Y = $(FOO\n$(info $(Y))\n", 1),
        ("Y = $(FOO\ninclude $(Y)\n", 1),
        ("Y = $(FOO\nZ = $(Y)\nall: $(Z)\n", 1),
        ("Y = $(FOO\nZ = $(Y)\nW = $(Z)\nall:\n\t@echo $(W)\n", 1),
        ("Y ?= $(FOO\nall: $(Y)\n", 1),
        ("Y = $(FOO\nY ?= fine\nall: $(Y)\n", 1),
        ("Y = $(FOO\nY += more\nall: $(Y)\n", 1),
        ("override Y = $(FOO\nY = fine\nall: $(Y)\n", 1),
        ("Y = $(FOO\nifdef FIXED\nY = fine\nendif\nall: $(Y)\n", 1),
        ("Y = $(FOO\nifeq (a,b)\nY = fine\nendif\nall: $(Y)\n", 1),
        ("all: dep\nall: Y = $(FOO\ndep: ; @echo $(Y)\n", 2),
        ("all: Y = $(FOO\nall:\n\t@echo $(Y)\n", 1),
        ("%.o: Y = $(FOO\nmain.o:\n\t@echo $(Y)\n", 1),
        (
            "main.o: Y = $(FOO\n%.o: Y = fine\nmain.o:\n\t@echo $(Y)\n",
            1,
        ),
        ("override Y = fine\nall: Y = $(FOO\nall:\n\t@echo $(Y)\n", 2),
        ("Y = $(FOO\nall: Y ?= fine\nall:\n\t@echo $(Y)\n", 1),
        ("all: Y ?= $(FOO\nall:\n\t@echo $(Y)\n", 1),
        ("Y = $(FOO\nall: Y = fine\nall other:\n\t@echo $(Y)\n", 1),
        (
            "all: Y = $(FOO\ndep: Y += x\nall: dep\n\t@echo ok\ndep:\n\t@echo $(Y)\n",
            1,
        ),
        ("define D\n$(BAR\nendef\nX := $(D)\n", 2),
        ("define D\n$(BAR\nendef\nall:\n\t@echo $(D)\n", 2),
        ("MAKECMDGOALS ?= $(FOO\nall:\n\t@echo $(MAKECMDGOALS)\n", 1),
        ("CC = $(FOO\nall:\n\t@echo $(CC)\n", 1),
        ("CC += $(FOO\nall:\n\t@echo $(CC)\n", 1),
        (
            "ifdef X\noverride Y = $(FOO\nendif\nY = fine\nall:\n\t@echo $(Y)\n",
            2,
        ),
    ];
    for (source, line) in expanded {
        assert_eq!(mk006_lines(source), [line], "{source:?}");
    }
}

#[test]
fn mk006_accepts_a_deferred_reference_gnu_make_never_expands() {
    let unexpanded = [
        "Y = $(FOO\ndefine D\n$(BAR\nendef\n",
        "Y = $(FOO\nY = fine\nall: $(Y)\n",
        "Y = $(FOO\nY := fine\nall: $(Y)\n",
        "X := $(Y)\nY = $(FOO\n",
        "all: $(Y)\nY = $(FOO\n",
        "Y = $(FOO\nifeq (a,a)\nY = fine\nendif\nall: $(Y)\n",
        "Y = fine\nY ?= $(FOO\nall: $(Y)\n",
        "Y = $(FOO\nZ = $(Y)\nZ := fine\nall: $(Z)\n",
        "Y = $(FOO\nY += more\n",
        "debug: OPTS = $(FOO\nrelease: ; @echo $(OPTS)\n",
        "X := ok\nall: X += $(FOO\nall: ; @:\n",
        "Y = $(FOO\nall: Y = fine\nall:\n\t@echo $(Y)\n",
        "Y = $(FOO\nall other: Y = fine\nall:\n\t@echo $(Y)\n",
        "override Y = $(FOO\nall: Y = fine\nall:\n\t@echo $(Y)\n",
        "Y = fine\nall: Y ?= $(FOO\nall:\n\t@echo $(Y)\n",
        "all: Y = $(FOO\nall: Y = fine\nall:\n\t@echo $(Y)\n",
        "all: override Y = fine\nall: Y = $(FOO\nall:\n\t@echo $(Y)\n",
        "all: Y = $(FOO\ndep: Y = fine\nall: dep\n\t@echo ok\ndep:\n\t@echo $(Y)\n",
        "%.o: Y = $(FOO\nmain.o: Y = fine\nmain.o:\n\t@echo $(Y)\n",
        "define D\n$(BAR\nendef\nD := fine\nall: $(D)\n",
        "CC ?= $(FOO\nall:\n\t@echo $(CC)\n",
        "CURDIR ?= $(FOO\nX := $(CURDIR)\n",
        "ifdef X\noverride Y = $(FOO\nendif\noverride Y = fine\nall:\n\t@echo $(Y)\n",
        "ifdef X\nY = $(FOO\nendif\nY = fine\nall:\n\t@echo $(Y)\n",
    ];
    for source in unexpanded {
        assert_eq!(mk006_lines(source), Vec::<usize>::new(), "{source:?}");
    }
}

#[test]
fn mk006_names_the_variable_whose_expansion_fails() {
    let source = "Y = $(FOO\nall: $(Y)\n";
    let diagnostics = InvalidSyntax.check(&parse(source), source);

    assert_eq!(
        diagnostics[0].message,
        "Unterminated variable reference: missing ')' (GNU Make fails when 'Y' is expanded)"
    );
}

#[test]
fn mk006_accepts_a_reference_in_a_branch_gnu_make_never_reads() {
    let never_read = [
        "Y = $(FOO\nifeq (a,b)\nX := $(Y)\nendif\n",
        "Y = $(FOO\nall:\nifeq (a,b)\n\t@echo $(Y)\nendif\n\t@echo ok\n",
        "Y = $(FOO\nifeq (a,b)\nall: $(Y)\nendif\n",
        "Y = $(FOO\nifeq (a,a)\nelse\nX := $(Y)\nendif\n",
    ];
    for source in never_read {
        assert_eq!(mk006_lines(source), Vec::<usize>::new(), "{source:?}");
    }
    assert_eq!(mk006_lines("Y = $(FOO\nifdef X\nX := $(Y)\nendif\n"), [1]);
    assert_eq!(
        mk006_lines("Y = $(FOO\nifeq (a,$(A))\nX := $(Y)\nendif\n"),
        [1]
    );
}

#[test]
fn mk006_follows_which_function_arguments_gnu_make_expands() {
    let never_expanded = [
        "X := $(if ,$(Y),ok)",
        "X := $(if x,ok,$(Y))",
        "X := $(or ok,$(Y))",
        "X := $(and ,$(Y))",
        "X := $(foreach v,,$(Y))",
        "X := $(if ,$(if $(Z),$(Y),ok),ok)",
        "X := ${if ,$(Y),ok}",
    ];
    for value in never_expanded {
        let source = format!("Y = $(FOO\n{value}\n");
        assert_eq!(mk006_lines(&source), Vec::<usize>::new(), "{value}");
    }
    let expanded = [
        "X := $(if $(Z),$(Y),ok)",
        "X := $(if x,$(Y),ok)",
        "X := $(if ,ok,$(Y))",
        "X := $(or ,$(Y))",
        "X := $(or $(Z),$(Y))",
        "X := $(and x,$(Y))",
        "X := $(foreach v,a b,$(Y))",
        "X := $(foreach v,$(Z),$(Y))",
        "X := $(if ,ok,$(if x,$(Y),ok))",
    ];
    for value in expanded {
        let source = format!("Y = $(FOO\n{value}\n");
        assert_eq!(mk006_lines(&source), [1], "{value}");
    }
}

#[test]
fn recipe_prefix_assignments_follow_gnu_make() {
    // `.RECIPEPREFIX` is defined empty at startup, so `?=` never assigns.
    assert_eq!(
        errors(".RECIPEPREFIX ?= >\nall:\n>echo hi\n"),
        [(missing_separator(None), 3, 1)]
    );
    // The first `+=` sets the prefix; a later one keeps it.
    assert!(errors(".RECIPEPREFIX += >\nall:\n>echo hi\n").is_empty());
    assert!(errors(".RECIPEPREFIX = >\n.RECIPEPREFIX += !\nall:\n>echo hi\n").is_empty());
    // An assignment in a branch GNU Make never reads is ignored.
    assert_eq!(
        errors("ifeq (a,b)\n.RECIPEPREFIX = >\nendif\nall:\n>echo hi\n"),
        [(missing_separator(None), 5, 1)]
    );
    assert!(errors("ifdef X\n.RECIPEPREFIX = >\nendif\nall:\n>echo hi\n").is_empty());
    // `=` keeps the raw text, so a leading reference makes `$` the prefix.
    assert_eq!(
        errors("X := >\n.RECIPEPREFIX = $(X)\nall:\n>echo hi\n"),
        [(missing_separator(None), 4, 1)]
    );
    assert!(errors("X := >\n.RECIPEPREFIX = $(X)\nall:\n$echo hi\n").is_empty());
    // An empty value restores the tab.
    assert!(errors(".RECIPEPREFIX = >\n.RECIPEPREFIX =\nall:\n\techo hi\n").is_empty());
    // A modifier may stand before the name.
    assert!(errors("export .RECIPEPREFIX = >\nall:\n>echo hi\n").is_empty());
    assert!(errors("override .RECIPEPREFIX = >\nall:\n>echo hi\n").is_empty());
    // Any other word before the name makes the line unparseable, so GNU Make
    // rejects it and goes on reading recipes with a tab.
    assert_eq!(
        errors("foo .RECIPEPREFIX = >\nall:\n\techo hi\n"),
        [(missing_separator(None), 1, 1)]
    );
}

#[test]
fn a_recipe_prefix_rumk_cannot_evaluate_proves_no_missing_separator() {
    // Make reads each of these files with a prefix Rumk cannot work out, so
    // neither indentation is provably wrong.
    let sources = [
        "P := >\n.RECIPEPREFIX := $(P)\nall:\n>echo hi\n",
        "P := >\n.RECIPEPREFIX := $(P)\nall:\n\techo hi\n",
        ".RECIPEPREFIX != printf %s '>'\nall:\n>echo hi\n",
        ".RECIPEPREFIX != printf %s '>'\nall:\n\techo hi\n",
        "define .RECIPEPREFIX\n>\nendef\nall:\n>echo hi\n",
        "define .RECIPEPREFIX\n>\nendef\nall:\n\techo hi\n",
        "ifdef X\n.RECIPEPREFIX = >\nendif\nall:\n\techo hi\n",
    ];
    for source in sources {
        assert!(errors(source).is_empty(), "{source:?}");
    }
}

#[test]
fn a_recipe_prefix_rumk_cannot_work_out_proves_no_broken_reference_in_a_recipe() {
    // Make expands the last line only where it reads it as a recipe, which the
    // prefix decides. The other reading is an assignment nothing expands, and
    // Make accepts the file, so neither reading can be held against it.
    let sources = [
        "ifdef X\n.RECIPEPREFIX = >\nendif\nall:;@true\n>FOO = $(\n",
        "P := >\n.RECIPEPREFIX := $(P)\nall:;@true\n\tFOO = $(\n",
    ];
    for source in sources {
        assert!(errors(source).is_empty(), "{source:?}");
    }
    // A prefix Rumk reads outright leaves one reading, and Make stops there.
    assert_eq!(
        errors(".RECIPEPREFIX = >\nall:;@true\n>FOO = $(\n"),
        [(unterminated(')', None, None), 3, 8)]
    );
    assert_eq!(
        errors("all:;@true\n\tFOO = $(\n"),
        [(unterminated(')', None, None), 2, 8)]
    );
}

#[test]
fn a_comment_after_a_define_header_is_not_part_of_it() {
    let source = "define FOO # := note\n$(BROKEN\nendef\nall: ; @echo ok\n";
    let makefile = parse(source);

    assert_eq!(mk006_lines(source), Vec::<usize>::new());
    assert_eq!(makefile.definitions[0].name, "FOO");
    assert_eq!(
        makefile.definitions[0].operator,
        AssignmentOperator::Recursive
    );
}

#[test]
fn a_modifier_without_an_assignment_is_a_missing_separator() {
    let rejected = [
        "private foo\n",
        "private\n",
        "override foo\n",
        "override\n",
        "override export FOO\n",
        "private export FOO\n",
    ];
    for source in rejected {
        assert_eq!(
            errors(source),
            [(missing_separator(None), 1, 1)],
            "{source:?}"
        );
    }
    let accepted = [
        "export foo bar\n",
        "unexport foo\n",
        "export\n",
        "unexport\n",
        "export private FOO\n",
        "unexport override BAR\n",
        "private define D\nendef\n",
        "override undefine Z\n",
        "private export X = 1\n",
    ];
    for source in accepted {
        assert!(errors(source).is_empty(), "{source:?}");
    }
}

#[test]
fn unbalanced_conditionals_are_reported() {
    fn unterminated_conditional(header: &str) -> SyntaxErrorKind {
        SyntaxErrorKind::UnterminatedConditional {
            conditional: header.to_string(),
        }
    }

    assert_eq!(
        errors("ifeq (a,a)\nall: ; @echo ok\n"),
        [(unterminated_conditional("ifeq (a,a)"), 1, 1)]
    );
    assert_eq!(
        errors("  ifdef X # comment\nall:\n"),
        [(unterminated_conditional("ifdef X"), 1, 3)]
    );
    assert_eq!(
        errors("ifdef A\nifdef B\n"),
        [
            (unterminated_conditional("ifdef A"), 1, 1),
            (unterminated_conditional("ifdef B"), 2, 1),
        ]
    );
    assert_eq!(
        errors("ifeq (a,a)\nendif\nendif\n"),
        [(SyntaxErrorKind::UnexpectedEndif, 3, 1)]
    );
    assert_eq!(
        errors("else\nall:\n"),
        [(SyntaxErrorKind::UnexpectedElse, 1, 1)]
    );
    assert_eq!(
        errors("ifeq (a,b)\nelse\nelse\nendif\n"),
        [(SyntaxErrorKind::SecondElse, 3, 1)]
    );
    assert_eq!(
        errors("ifeq (a,b)\nelse\nelse ifeq (a,a)\nendif\n"),
        [(SyntaxErrorKind::SecondElse, 3, 1)]
    );
    assert!(errors("ifeq (a,a)\nelse ifeq (b,b)\nelse ifdef X\nelse\nendif\n").is_empty());
    // A conditional written without the space still opens one, so its
    // `endif` is not stray and its absence is reported.
    assert_eq!(
        errors("ifeq(a,b)\n"),
        [
            (
                missing_separator(Some(SeparatorHint::ConditionalWithoutSpace)),
                1,
                1
            ),
            (unterminated_conditional("ifeq(a,b)"), 1, 1),
        ]
    );
}

#[test]
fn conditional_nesting_is_followed_in_branches_gnu_make_never_reads() {
    fn unterminated_conditional(header: &str) -> SyntaxErrorKind {
        SyntaxErrorKind::UnterminatedConditional {
            conditional: header.to_string(),
        }
    }

    assert_eq!(
        errors("ifeq (a,b)\nendif\nendif\n"),
        [(SyntaxErrorKind::UnexpectedEndif, 3, 1)]
    );
    assert_eq!(
        errors("ifeq (a,b)\nifdef X\nelse\nelse\nendif\nendif\n"),
        [(SyntaxErrorKind::SecondElse, 4, 1)]
    );
    // A define in such a branch still swallows the lines up to `endef`.
    assert_eq!(
        errors("ifeq (a,b)\ndefine X\nendif\nall: ; @echo ok\n"),
        [(unterminated_conditional("ifeq (a,b)"), 1, 1)]
    );
    // GNU Make 4.3 and later accept an `endef` there.
    assert!(errors("ifeq (a,b)\nendef\nendif\n").is_empty());
}

#[test]
fn ifeq_operands_keep_the_blanks_at_the_parentheses() {
    // The blanks around the comma are dropped, so the branch is read.
    assert_eq!(
        errors("ifeq (x \t, x)\n\t@echo hi\nendif\nall: ; @echo ok\n"),
        [(SyntaxErrorKind::RecipeBeforeTarget, 2, 1)]
    );
    // The blanks after `(` and before `)` belong to the operands.
    assert!(errors("ifeq (x,x )\n\t@echo hi\nendif\nall: ; @echo ok\n").is_empty());
    assert!(errors("ifeq ( x,x)\n\t@echo hi\nendif\nall: ; @echo ok\n").is_empty());
    assert_eq!(
        errors("ifneq (x,x )\n\t@echo hi\nendif\nall: ; @echo ok\n"),
        [(SyntaxErrorKind::RecipeBeforeTarget, 2, 1)]
    );
}

#[test]
fn parentheses_inside_a_reference_hold_no_separator() {
    // A bare `(` inside a function call nests, so the `:` after `(a)` is
    // still inside the call and the line may expand into a rule.
    assert!(errors("$(if x,(a):b)\nall: ; @echo ok\n").is_empty());
    assert!(errors("$(if x,(a)=b)\nall: ; @echo ok\n").is_empty());
    assert!(errors("all: $(if x,(a):b) ; @echo ok\n").is_empty());
    // A `$` before any other character opens a reference to that
    // character, so `$:` is no separator and `$#` starts no comment. A
    // line holding a reference may still expand into a statement, so the
    // recipe below is what is reported.
    assert_eq!(
        errors("foo$:bar\n\t@echo hi\n"),
        [(SyntaxErrorKind::RecipeBeforeTarget, 2, 1)]
    );
    assert_eq!(
        errors("FOO = a$#b $(X\n"),
        [(unterminated(')', None, Some("FOO")), 1, 12)]
    );
}

#[test]
fn mk006_reports_an_exported_broken_variable_when_a_recipe_runs() {
    // Every rule that runs a command is handed the exported value.
    assert_eq!(
        mk006_lines("BROKEN = $(X\nexport BROKEN\nall: ; @echo ok\n"),
        [1]
    );
    assert_eq!(mk006_lines("export BROKEN = $(X\nall: ; @echo ok\n"), [1]);
    assert_eq!(mk006_lines("BROKEN = $(X\nexport\nall: ; @echo ok\n"), [1]);
    assert_eq!(
        mk006_lines("BROKEN = $(X\n.EXPORT_ALL_VARIABLES:\nunexport\nall: ; @echo ok\n"),
        [1]
    );
    assert_eq!(
        mk006_lines("all: ; @echo ok\nexport private BROKEN\nBROKEN = $(X\n"),
        [3]
    );
    // The last directive naming the variable decides, and a bare
    // `unexport` cancels only a bare `export`.
    assert_eq!(
        mk006_lines("BROKEN = $(X\nexport BROKEN\nunexport BROKEN\nall: ; @echo ok\n"),
        Vec::<usize>::new()
    );
    assert_eq!(
        mk006_lines("BROKEN = $(X\nexport BROKEN\nunexport\nall: ; @echo ok\n"),
        [1]
    );
    assert_eq!(
        mk006_lines("BROKEN = $(X\nexport\nunexport BROKEN\nall: ; @echo ok\n"),
        Vec::<usize>::new()
    );
    assert_eq!(
        mk006_lines("BROKEN = $(X\nexport\nunexport\nall: ; @echo ok\n"),
        Vec::<usize>::new()
    );
    // Only a name a shell accepts is exported by a bare `export`.
    assert_eq!(
        mk006_lines("BROKEN.X = $(X\nexport\nall: ; @echo ok\n"),
        Vec::<usize>::new()
    );
    // A rule whose recipe runs nothing builds no environment.
    assert_eq!(
        mk006_lines("BROKEN = $(X\nexport BROKEN\nall: ;\n"),
        Vec::<usize>::new()
    );
    assert_eq!(
        mk006_lines("BROKEN = $(X\nexport BROKEN\nall:\n\t\n"),
        Vec::<usize>::new()
    );
    assert_eq!(
        mk006_lines("BROKEN = $(X\nexport BROKEN\n"),
        Vec::<usize>::new()
    );
    // A rebinding before the recipe runs replaces the broken value.
    assert_eq!(
        mk006_lines("BROKEN = $(X\nexport BROKEN\nBROKEN = ok\nall: ; @echo ok\n"),
        Vec::<usize>::new()
    );
    assert_eq!(
        mk006_lines("BROKEN = $(X\nexport BROKEN\nall: BROKEN = ok\nall: ; @echo ok\n"),
        Vec::<usize>::new()
    );
    // A directive in a branch GNU Make never reads exports nothing, and a
    // computed name is not followed.
    assert_eq!(
        mk006_lines("BROKEN = $(X\nifeq (a,b)\nexport BROKEN\nendif\nall: ; @echo ok\n"),
        Vec::<usize>::new()
    );
    assert_eq!(
        mk006_lines("BROKEN = $(X\nNAME = BROKEN\nexport $(NAME)\nall: ; @echo ok\n"),
        Vec::<usize>::new()
    );
}
