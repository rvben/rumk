use crate::diagnostic::Diagnostic;
use crate::rules::RULE_IDS;
use std::collections::BTreeSet;
use std::fmt;

/// A `# rumk-` comment Rumk cannot act on: a directive or a rule it does not
/// know. It is an error rather than a comment to pass over, because a
/// suppression with a typo in it would otherwise quietly suppress nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidDirective {
    /// The 1-based line the directive is written on.
    pub line: usize,
    pub message: String,
}

impl fmt::Display for InvalidDirective {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for InvalidDirective {}

pub fn apply_inline_suppressions(
    content: &str,
    diagnostics: Vec<Diagnostic>,
) -> Result<Vec<Diagnostic>, InvalidDirective> {
    let mut disabled = BTreeSet::new();
    let mut pending = BTreeSet::new();
    let mut suppressions = Vec::new();

    for (index, line) in content.lines().enumerate() {
        let mut suppressed = disabled.clone();
        suppressed.append(&mut pending);
        let mut next_pending = BTreeSet::new();

        if !line.starts_with('\t') {
            let directive = line
                .trim_start()
                .strip_prefix('#')
                .map(str::trim_start)
                .and_then(|directive| directive.strip_prefix("rumk-"));
            if let Some(directive) = directive {
                let (command, arguments) = directive
                    .split_once(char::is_whitespace)
                    .map_or((directive, ""), |(command, arguments)| {
                        (command, arguments.trim())
                    });
                let invalid = |message| InvalidDirective {
                    line: index + 1,
                    message,
                };
                let rules = parse_rules(arguments).map_err(invalid)?;
                match command {
                    "disable" => {
                        disabled.extend(rules);
                        suppressed.extend(disabled.iter().cloned());
                    }
                    "enable" => {
                        for rule in rules {
                            disabled.remove(&rule);
                            suppressed.remove(&rule);
                        }
                    }
                    "disable-line" => suppressed.extend(rules),
                    "disable-next-line" => next_pending.extend(rules),
                    _ => {
                        return Err(invalid(format!(
                            "Unknown inline configuration directive: rumk-{command}"
                        )));
                    }
                }
            }
        }

        suppressions.push(suppressed);
        pending = next_pending;
    }

    Ok(diagnostics
        .into_iter()
        .filter(|diagnostic| {
            !suppressions
                .get(diagnostic.line.saturating_sub(1))
                .is_some_and(|rules| rules.contains(&diagnostic.rule_id))
        })
        .collect())
}

fn parse_rules(arguments: &str) -> Result<BTreeSet<String>, String> {
    if arguments.is_empty() || arguments.eq_ignore_ascii_case("all") {
        return Ok(RULE_IDS.iter().map(|rule| (*rule).to_string()).collect());
    }

    arguments
        .split([',', ' ', '\t'])
        .filter(|rule| !rule.is_empty())
        .map(|rule| {
            let canonical = rule.to_ascii_uppercase();
            if RULE_IDS.contains(&canonical.as_str()) {
                Ok(canonical)
            } else {
                Err(format!("Unknown rule in inline configuration: {rule}"))
            }
        })
        .collect()
}
