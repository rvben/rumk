#[cfg(test)]
mod tests {
    use rumk::logical::ConditionalKind;
    use rumk::parser::{parse, AssignmentOperator};

    #[test]
    fn test_parse_simple_rule() {
        let content = r#"
target: dependency
	command
"#;
        let makefile = parse(content).unwrap();
        assert_eq!(makefile.rules.len(), 1);
        assert_eq!(makefile.rules[0].targets, vec!["target"]);
        assert_eq!(makefile.rules[0].recipes.len(), 1);
        assert_eq!(makefile.rules[0].recipes[0].command, "command");
    }

    #[test]
    fn test_parse_variable() {
        let content = "FOO = bar";
        let makefile = parse(content).unwrap();
        assert_eq!(makefile.variables.len(), 1);
        assert!(makefile.variables.contains_key("FOO"));
        assert_eq!(makefile.variables["FOO"].value, "bar");
    }

    #[test]
    fn test_parse_phony() {
        let content = ".PHONY: clean test";
        let makefile = parse(content).unwrap();
        assert_eq!(makefile.phonies.len(), 2);
        assert!(makefile.phonies.contains(&"clean".to_string()));
        assert!(makefile.phonies.contains(&"test".to_string()));
    }

    #[test]
    fn parses_immediate_variable_assignments_as_variables() {
        let content = "FOO := /usr/local/bin\n";
        let makefile = parse(content).unwrap();

        assert!(makefile.rules.is_empty());
        assert_eq!(makefile.variables["FOO"].value, "/usr/local/bin");
        assert_eq!(makefile.variables["FOO"].line, 1);
    }

    #[test]
    fn preserves_the_start_line_for_rules_and_multiline_variables() {
        let content = "FOO = one \\\n  two\nclean:\n\ttrue\n\nnext:\n";
        let makefile = parse(content).unwrap();

        assert_eq!(makefile.variables["FOO"].line, 1);
        assert_eq!(makefile.rules[0].line, 3);
        assert_eq!(makefile.rules[1].line, 6);
    }

    #[test]
    fn parses_dot_prefixed_rules_and_their_recipes() {
        let content = ".DEFAULT:\n    /usr/bin/true\n";
        let makefile = parse(content).unwrap();

        assert_eq!(makefile.rules.len(), 1);
        assert_eq!(makefile.rules[0].targets, vec![".DEFAULT"]);
        assert_eq!(makefile.rules[0].recipes.len(), 1);
        assert_eq!(makefile.rules[0].recipes[0].line, 2);
    }

    #[test]
    fn preserves_assignment_order_flavor_and_modifiers() {
        let content = concat!(
            "export override CC := clang\n",
            "CC += -pthread\n",
            "private OUTPUT ?= app\n",
            "POSIX ::= expanded\n",
            "IMMEDIATE :::= value\n",
        );
        let makefile = parse(content).unwrap();

        assert_eq!(makefile.assignments.len(), 5);
        assert_eq!(makefile.assignments[0].name, "CC");
        assert_eq!(makefile.assignments[0].operator, AssignmentOperator::Simple);
        assert!(makefile.assignments[0].modifiers.export);
        assert!(makefile.assignments[0].modifiers.override_);
        assert_eq!(makefile.assignments[1].operator, AssignmentOperator::Append);
        assert_eq!(makefile.variables["CC"].value, "-pthread");
        assert_eq!(
            makefile.assignments[2].operator.as_str(),
            AssignmentOperator::Conditional.as_str()
        );
        assert!(makefile.assignments[2].modifiers.private);
        assert_eq!(
            makefile.assignments[3].operator,
            AssignmentOperator::SimplePosix
        );
        assert_eq!(
            makefile.assignments[4].operator,
            AssignmentOperator::ImmediateRecursive
        );
    }

    #[test]
    fn models_rule_separators_and_prerequisite_classes() {
        let content = "one\\ two archive &: input.o lib.o | generated stamp\n";
        let makefile = parse(content).unwrap();
        let rule = &makefile.rules[0];

        assert_eq!(rule.targets, ["one two", "archive"]);
        assert_eq!(rule.prerequisites, ["input.o", "lib.o"]);
        assert_eq!(rule.order_only_prerequisites, ["generated", "stamp"]);
        assert!(rule.grouped);
        assert!(!rule.double_colon);
    }

    #[test]
    fn parses_double_colon_and_inline_recipes() {
        let content = "clean:: ; -@+rm -rf build # handled by the shell\n";
        let makefile = parse(content).unwrap();
        let rule = &makefile.rules[0];
        let recipe = &rule.recipes[0];

        assert!(rule.double_colon);
        assert!(recipe.inline);
        assert!(recipe.silent);
        assert!(recipe.ignore_errors);
        assert!(recipe.recursive);
        assert_eq!(recipe.command, "rm -rf build # handled by the shell");
        assert_eq!(recipe.column, 14);
    }

    #[test]
    fn honors_custom_recipe_prefixes() {
        let content = ".RECIPEPREFIX := >\nall:\n>@echo ok\n";
        let makefile = parse(content).unwrap();
        let recipe = &makefile.rules[0].recipes[0];

        assert_eq!(recipe.indentation, ">");
        assert!(recipe.silent);
        assert_eq!(recipe.command, "echo ok");
    }

    #[test]
    fn comments_between_a_rule_and_recipe_are_not_commands() {
        let content = "all:\n  # explanation\n\ttrue\n";
        let makefile = parse(content).unwrap();

        assert_eq!(makefile.rules[0].recipes.len(), 1);
        assert_eq!(makefile.rules[0].recipes[0].command, "true");
    }

    #[test]
    fn indented_conditionals_after_a_rule_are_not_parsed_as_recipes() {
        let content = "all:\n\t@echo all\n  ifeq ($(MODE),debug)\n  CFLAGS := -g\n  endif\n";

        let makefile = parse(content).unwrap();

        assert_eq!(makefile.rules.len(), 1);
        assert_eq!(makefile.rules[0].recipes.len(), 1);
        assert_eq!(makefile.conditionals.len(), 2);
        assert_eq!(makefile.conditionals[0].kind, ConditionalKind::Ifeq);
        assert_eq!(makefile.conditionals[1].kind, ConditionalKind::Endif);
    }

    #[test]
    fn parses_expression_aware_continued_rules() {
        let content = concat!(
            "$(call target,a:b=c): $(call deps,x:y=z) \\\n  generated.o | stamp\n",
            "\tprintf '%s\\n' one \\\n\t  two\n",
        );
        let makefile = parse(content).unwrap();
        let rule = &makefile.rules[0];

        assert_eq!(rule.targets, ["$(call target,a:b=c)"]);
        assert_eq!(rule.prerequisites, ["$(call deps,x:y=z)", "generated.o"]);
        assert_eq!(rule.order_only_prerequisites, ["stamp"]);
        assert_eq!(rule.recipes.len(), 1);
        assert_eq!(rule.recipes[0].line, 3);
        assert_eq!(rule.recipes[0].end_line, 4);
        assert!(rule.recipes[0].command.contains("\\\n\t  two"));
    }

    #[test]
    fn models_includes_conditionals_and_definitions() {
        let content = concat!(
            "include base.mk \\\n  $(wildcard config/*.mk)\n",
            "-include local.mk\n",
            "ifdef DEBUG\n",
            "override define banner :=\n",
            "target: is data, not a rule\n",
            "endef\n",
            "endif\n",
        );
        let makefile = parse(content).unwrap();

        assert_eq!(
            makefile.includes[0].paths,
            ["base.mk", "$(wildcard config/*.mk)"]
        );
        assert!(!makefile.includes[0].optional);
        assert!(makefile.includes[1].optional);
        assert_eq!(makefile.conditionals.len(), 2);
        assert_eq!(makefile.definitions.len(), 1);
        assert_eq!(makefile.definitions[0].name, "banner");
        assert_eq!(makefile.definitions[0].value, "target: is data, not a rule");
        assert!(makefile.definitions[0].modifiers.override_);
        assert!(makefile.rules.is_empty());
        assert_eq!(
            makefile.variables["banner"].value,
            makefile.definitions[0].value
        );
    }

    #[test]
    fn strips_inline_comments_from_include_paths() {
        let makefile = parse("include mk/common.mk\t# shared settings\n").unwrap();

        assert_eq!(makefile.includes.len(), 1);
        assert_eq!(makefile.includes[0].paths, ["mk/common.mk"]);
    }

    #[test]
    fn models_static_patterns_and_target_specific_variables() {
        let content = concat!(
            "objects: %.o: %.c | generated\n",
            "app debug: private CFLAGS += -g\n",
        );
        let makefile = parse(content).unwrap();

        assert_eq!(makefile.rules[0].target_pattern.as_deref(), Some("%.o"));
        assert_eq!(makefile.rules[0].prerequisites, ["%.c"]);
        assert_eq!(makefile.rules[0].order_only_prerequisites, ["generated"]);
        let assignment = makefile.rules[1].target_assignment.as_ref().unwrap();
        assert_eq!(assignment.name, "CFLAGS");
        assert_eq!(assignment.value, "-g");
        assert!(assignment.modifiers.private);
        assert_eq!(
            assignment.scope,
            rumk::parser::VariableScope::TargetSpecific(vec!["app".into(), "debug".into()])
        );
    }

    #[test]
    fn recognizes_oneshell_mode() {
        let makefile = parse(".ONESHELL:\nall:\n\tcd build\n\tprintf '%s\\n' done\n").unwrap();

        assert!(makefile.oneshell);
        assert_eq!(makefile.rules[1].recipes.len(), 2);
    }
}
