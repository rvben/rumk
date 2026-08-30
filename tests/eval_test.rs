use std::collections::BTreeMap;

use rumk::eval::{BlockedReason, EvaluationLocation, Evaluator, Truth, VariableFlavor};
use rumk::logical::ConditionalKind;
use rumk::parser::{parse, VariableScope};
use rumk::project::SourceId;

fn assignment(source: &str) -> rumk::parser::Variable {
    parse(source)
        .unwrap()
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
    let makefile = parse("strip := preserved\nshell := harmless\n").unwrap();
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
