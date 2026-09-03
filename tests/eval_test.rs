use std::collections::BTreeMap;

use rumk::eval::{
    BlockedReason, EvaluationLocation, Evaluator, Truth, UndefinedName, VariableFlavor,
};
use rumk::logical::ConditionalKind;
use rumk::parser::{parse, VariableScope};
use rumk::project::SourceId;

fn assignment(source: &str) -> rumk::parser::Variable {
    parse(source)
        .assignments
        .into_iter()
        .find(|variable| variable.scope == VariableScope::Global)
        .unwrap()
}

fn location(line: usize) -> EvaluationLocation {
    EvaluationLocation {
        source: SourceId(0),
        line,
    }
}

/// A name read where Make had read `definitions_read` definitions, so a
/// definition it reads after that one is a definition the reading came before.
fn waited_on(name: &str, definitions_read: usize) -> UndefinedName {
    UndefinedName {
        name: name.to_string(),
        definitions_read,
    }
}

#[test]
fn reads_a_simple_assignment_that_waited_on_a_name_as_the_value_make_computed() {
    let mut evaluator = Evaluator::new(&BTreeMap::new());
    evaluator.assign(&assignment("DIR := $(LATER)\n"), location(1), Truth::True);

    assert_eq!(
        evaluator.expand_as_undefined("$(DIR)/x.mk"),
        Some(("/x.mk".to_string(), vec![waited_on("LATER", 1)]))
    );

    // An assignment this expression never reads says nothing about it.
    evaluator.assign(
        &assignment("OTHER := $(ELSEWHERE)\n"),
        location(2),
        Truth::True,
    );

    assert_eq!(
        evaluator.expand_as_undefined("$(DIR)/x.mk"),
        Some(("/x.mk".to_string(), vec![waited_on("LATER", 1)]))
    );

    // Make computes a simple assignment where it is written, so a value given
    // to LATER below never reaches what DIR already holds, and the definition
    // that gives it one is still a definition DIR was read before.
    evaluator.assign(&assignment("LATER := sub\n"), location(3), Truth::True);

    assert_eq!(
        evaluator.expand_as_undefined("$(DIR)/x.mk"),
        Some(("/x.mk".to_string(), vec![waited_on("LATER", 1)]))
    );
    assert_eq!(evaluator.definition_after("LATER", 1), Some(location(3)));
}

#[test]
fn reads_a_name_as_waiting_on_the_definitions_make_reads_after_it() {
    let mut evaluator = Evaluator::new(&BTreeMap::new());
    evaluator.assign(&assignment("DIR := sub\n"), location(1), Truth::True);
    evaluator.undefine("DIR", Truth::True);

    // DIR has no value here, and the definition on line 1 is not why: Make
    // read that one and gave the value back. Nothing else has been read, so
    // there is no definition this expansion waits on.
    assert_eq!(
        evaluator.expand_as_undefined("$(DIR)/x.mk"),
        Some(("/x.mk".to_string(), vec![waited_on("DIR", 1)]))
    );
    assert_eq!(evaluator.definition_after("DIR", 1), None);

    // A definition Make reads afterwards is one that expansion came before.
    evaluator.assign(&assignment("DIR := other\n"), location(4), Truth::Unknown);
    assert_eq!(evaluator.definition_after("DIR", 1), Some(location(4)));

    // Reading line 1 again, as an include read twice makes Make do, is another
    // definition, and one an expansion in between came before.
    evaluator.undefine("DIR", Truth::True);
    assert_eq!(
        evaluator.expand_as_undefined("$(DIR)/x.mk"),
        Some(("/x.mk".to_string(), vec![waited_on("DIR", 2)]))
    );
    assert_eq!(evaluator.definition_after("DIR", 2), None);

    evaluator.assign(&assignment("DIR := sub\n"), location(1), Truth::True);
    assert_eq!(evaluator.definition_after("DIR", 2), Some(location(1)));
}

#[test]
fn keeps_the_earliest_reading_of_a_name_two_assignments_waited_on() {
    let mut evaluator = Evaluator::new(&BTreeMap::new());
    evaluator.assign(&assignment("A := $(X)\n"), location(1), Truth::True);
    evaluator.assign(&assignment("X := prefix\n"), location(2), Truth::True);
    evaluator.undefine("X", Truth::True);
    evaluator.assign(&assignment("B := $(X)\n"), location(4), Truth::True);

    // Both assignments read X while it had no value, and only the earlier
    // reading came before the definition on line 2. An expression behind both
    // of them is an expression that reading reached.
    assert_eq!(
        evaluator.expand_as_undefined("$(B)$(A)config.mk"),
        Some(("config.mk".to_string(), vec![waited_on("X", 1)]))
    );
    assert_eq!(evaluator.definition_after("X", 1), Some(location(2)));
}

/// Reading an assignment Rumk cannot expand must cost the assignment, not the
/// whole evaluation. Reading each of them against a copy of every variable
/// read so far takes time in the square of their number, which a generated
/// Makefile of a few thousand lines turns into half a minute.
#[test]
fn reads_an_unexpandable_assignment_without_rereading_every_variable() {
    const ASSIGNMENTS: usize = 6400;
    let mut evaluator = Evaluator::new(&BTreeMap::new());
    let assignments: Vec<_> = (0..ASSIGNMENTS)
        .map(|index| assignment(&format!("V{index} := $(U{index}) tail{index}\n")))
        .collect();

    let started = std::time::Instant::now();
    for (index, variable) in assignments.iter().enumerate() {
        evaluator.assign(variable, location(index + 1), Truth::True);
    }
    let elapsed = started.elapsed();

    // Every one of them holds the value Make computed for it, so the reading
    // did happen; it just did not read the rest of the file again.
    assert_eq!(
        evaluator.expand_as_undefined("$(V6399)"),
        Some((
            " tail6399".to_string(),
            vec![waited_on("U6399", ASSIGNMENTS)]
        ))
    );
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "reading {ASSIGNMENTS} unexpandable assignments took {elapsed:?}"
    );
}

#[test]
fn evaluates_recursive_simple_conditional_and_append_assignments() {
    let mut evaluator = Evaluator::new(&BTreeMap::new());
    evaluator.assign(&assignment("BASE = src\n"), location(1), Truth::True);
    evaluator.assign(
        &assignment("FILES := $(BASE)/a.c\n"),
        location(2),
        Truth::True,
    );
    evaluator.assign(&assignment("BASE = lib\n"), location(3), Truth::True);
    evaluator.assign(
        &assignment("FILES += $(BASE)/b.c\n"),
        location(4),
        Truth::True,
    );
    evaluator.assign(&assignment("BASE ?= ignored\n"), location(5), Truth::True);

    assert_eq!(evaluator.expand("$(BASE)").as_known(), Some("lib"));
    assert_eq!(
        evaluator.expand("$(FILES)").as_known(),
        Some("src/a.c lib/b.c")
    );
    assert_eq!(evaluator.flavor("BASE"), Some(VariableFlavor::Recursive));
    assert_eq!(evaluator.flavor("FILES"), Some(VariableFlavor::Simple));
}

#[test]
fn protects_predefined_values_unless_override_is_explicit() {
    let mut evaluator = Evaluator::new(&BTreeMap::from([("MODE".into(), "ci".into())]));
    evaluator.assign(&assignment("MODE := local\n"), location(1), Truth::True);
    assert_eq!(evaluator.expand("$(MODE)").as_known(), Some("ci"));

    evaluator.assign(
        &assignment("override MODE := forced\n"),
        location(2),
        Truth::True,
    );
    assert_eq!(evaluator.expand("$(MODE)").as_known(), Some("forced"));
}

#[test]
fn substitutes_nothing_the_way_gnu_make_substitutes_it() {
    let evaluator = Evaluator::default();

    // With nothing to look for Make appends the replacement once, to the
    // whole text rather than to each word in it.
    assert_eq!(evaluator.expand("$(subst ,z,abc)").as_known(), Some("abcz"));
    assert_eq!(evaluator.expand("$(subst ,z,a b)").as_known(), Some("a bz"));
    assert_eq!(evaluator.expand("$(subst ,z,)").as_known(), Some("z"));
    assert_eq!(evaluator.expand("$(subst ,,abc)").as_known(), Some("abc"));
    assert_eq!(evaluator.expand("$(subst a,,aa)").as_known(), Some(""));
    // A pattern with nothing in it matches no word at all.
    assert_eq!(
        evaluator.expand("$(patsubst ,z,abc)").as_known(),
        Some("abc")
    );
}

#[test]
fn expands_nested_safe_functions_and_preserves_trace() {
    let mut evaluator = Evaluator::new(&BTreeMap::new());
    evaluator.assign(
        &assignment("SOURCES = src/a.c  src/b.c\n"),
        location(1),
        Truth::True,
    );

    let expansion = evaluator.expand("$(patsubst %.c,%.o,$(strip $(SOURCES)))");

    assert_eq!(expansion.as_known(), Some("src/a.o src/b.o"));
    assert_eq!(expansion.trace[0].variable, "SOURCES");
    assert_eq!(expansion.trace[0].origin, Some(location(1)));
}

#[test]
fn refuses_side_effecting_dynamic_and_recursive_expansions() {
    let mut evaluator = Evaluator::new(&BTreeMap::new());
    evaluator.assign(&assignment("LOOP = $(LOOP)\n"), location(1), Truth::True);

    assert!(evaluator
        .expand("$(shell touch forbidden)")
        .blocked
        .contains(&BlockedReason::UnsafeFunction("shell".into())));
    assert!(evaluator
        .expand("$($(NAME))")
        .blocked
        .iter()
        .any(|reason| matches!(reason, BlockedReason::DynamicVariableName(_))));
    assert!(evaluator
        .expand("$(LOOP)")
        .blocked
        .contains(&BlockedReason::RecursiveReference("LOOP".into())));
}

#[test]
fn evaluates_known_conditionals_and_keeps_external_inputs_unknown() {
    let mut evaluator = Evaluator::new(&BTreeMap::new());
    evaluator.assign(&assignment("MODE := debug\n"), location(1), Truth::True);

    assert_eq!(
        evaluator.condition(ConditionalKind::Ifeq, "($(MODE),debug)"),
        Truth::True
    );
    assert_eq!(
        evaluator.condition(ConditionalKind::Ifneq, "'$(MODE)' 'release'"),
        Truth::True
    );
    assert_eq!(
        evaluator.condition(ConditionalKind::Ifdef, "MODE"),
        Truth::True
    );
    assert_eq!(
        evaluator.condition(ConditionalKind::Ifdef, "FROM_ENV"),
        Truth::Unknown
    );
}

#[test]
fn indeterminate_assignments_poison_previous_values() {
    let mut evaluator = Evaluator::new(&BTreeMap::new());
    evaluator.assign(&assignment("MODE := debug\n"), location(1), Truth::True);
    evaluator.assign(
        &assignment("MODE := release\n"),
        location(2),
        Truth::Unknown,
    );

    assert!(evaluator.expand("$(MODE)").as_known().is_none());
    assert_eq!(evaluator.flavor("MODE"), Some(VariableFlavor::Unknown));
}

#[test]
fn function_names_without_arguments_remain_ordinary_variables() {
    let makefile = parse("strip := preserved\nshell := harmless\n");
    let mut evaluator = Evaluator::default();
    for variable in &makefile.assignments {
        evaluator.assign(
            variable,
            EvaluationLocation {
                source: SourceId(0),
                line: variable.line,
            },
            Truth::True,
        );
    }

    assert_eq!(evaluator.expand("$(strip)").as_known(), Some("preserved"));
    assert_eq!(evaluator.expand("$(shell)").as_known(), Some("harmless"));
}

#[test]
fn expands_suffix_and_pattern_substitution_references() {
    let mut evaluator = Evaluator::default();
    evaluator.assign(
        &assignment("SOURCES = src/one.c src/two.cc README\n"),
        location(1),
        Truth::True,
    );

    let suffix = evaluator.expand("$(SOURCES:.c=.o)");
    let pattern = evaluator.expand("$(SOURCES:src/%.c=build/%.o)");

    assert_eq!(suffix.as_known(), Some("src/one.o src/two.cc README"));
    assert_eq!(pattern.as_known(), Some("build/one.o src/two.cc README"));
    assert_eq!(suffix.trace[0].variable, "SOURCES");
}

#[test]
fn expands_word_path_and_join_functions() {
    let evaluator = Evaluator::default();

    assert_eq!(
        evaluator.expand("$(word 2,one two three)").as_known(),
        Some("two")
    );
    assert_eq!(
        evaluator
            .expand("$(wordlist 2,3,one two three four)")
            .as_known(),
        Some("two three")
    );
    assert_eq!(
        evaluator.expand("$(dir src/main.c README)").as_known(),
        Some("src/ ./")
    );
    assert_eq!(
        evaluator
            .expand("$(notdir src/main.c README trailing/)")
            .as_known(),
        Some("main.c README ")
    );
    assert_eq!(
        evaluator
            .expand("$(suffix src/main.c archive.tar.gz README)")
            .as_known(),
        Some(".c .gz")
    );
    assert_eq!(
        evaluator
            .expand("$(basename src/main.c archive.tar.gz README)")
            .as_known(),
        Some("src/main archive.tar README")
    );
    assert_eq!(
        evaluator.expand("$(join a b c,.1 .2)").as_known(),
        Some("a.1 b.2 c")
    );
}

#[test]
fn lazy_functions_never_expand_unselected_unsafe_branches() {
    let evaluator = Evaluator::default();

    assert_eq!(
        evaluator
            .expand("$(if yes,safe,$(shell touch forbidden))")
            .as_known(),
        Some("safe")
    );
    assert_eq!(
        evaluator
            .expand("$(or selected,$(shell touch forbidden))")
            .as_known(),
        Some("selected")
    );
    assert_eq!(
        evaluator
            .expand("$(and ,$(shell touch forbidden))")
            .as_known(),
        Some("")
    );
    assert!(evaluator
        .expand("$(if $(UNKNOWN),yes,no)")
        .blocked
        .contains(&BlockedReason::UndefinedVariable("UNKNOWN".into())));
}

#[test]
fn a_bare_parenthesis_inside_a_function_call_nests_the_way_gnu_make_nests_it() {
    let mut evaluator = Evaluator::new(&BTreeMap::new());
    evaluator.assign(&assignment("A := 1\n"), location(1), Truth::True);

    assert_eq!(
        evaluator.expand("$(if $(A),(y) z,n)").as_known(),
        Some("(y) z")
    );
    assert_eq!(evaluator.expand("$(if ,(a),b)").as_known(), Some("b"));
    // The `(` opens a level the final `)` closes, so the call never ends,
    // which is the "unterminated call to function" GNU Make reports.
    assert!(evaluator
        .expand("$(subst (,[,a(b)")
        .blocked
        .contains(&BlockedReason::MalformedExpansion));
}
