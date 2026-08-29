#[cfg(test)]
mod tests {
    use rumk::parser::parse;

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
}
