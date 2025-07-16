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
        assert_eq!(makefile.rules[0].prerequisites, vec!["dependency"]);
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
}
