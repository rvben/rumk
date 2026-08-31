pub(crate) const DEFAULT_PHONY_LINE_LENGTH: usize = 120;
pub(crate) const COMMON_PHONY_TARGETS: &[&str] =
    &["all", "clean", "test", "check", "install", "build", "help"];

const CONTINUATION_INDENT: &str = "        ";

pub(crate) fn format_continued_declaration(
    names: &[String],
    comment_suffix: &str,
    line_ending: &str,
    max_length: usize,
) -> String {
    let mut lines = Vec::new();
    let mut line = String::from(".PHONY:");

    for (index, name) in names.iter().enumerate() {
        let is_last = index + 1 == names.len();
        let suffix = if is_last { comment_suffix } else { " \\" };
        let candidate_length = line.chars().count()
            + usize::from(!line.ends_with(' '))
            + name.chars().count()
            + suffix.chars().count();

        if line != ".PHONY:" && candidate_length > max_length {
            line.push_str(" \\");
            lines.push(line);
            line = String::from(CONTINUATION_INDENT);
        }

        if !line.ends_with(' ') {
            line.push(' ');
        }
        line.push_str(name);
    }

    line.push_str(comment_suffix);
    lines.push(line);
    lines.join(line_ending)
}

pub(crate) fn preferred_line_ending(content: &str, line: usize) -> &'static str {
    match content.split_inclusive('\n').nth(line.saturating_sub(1)) {
        Some(source_line) if source_line.ends_with("\r\n") => "\r\n",
        Some(source_line) if source_line.ends_with('\n') => "\n",
        _ if content.contains("\r\n") => "\r\n",
        _ => "\n",
    }
}
