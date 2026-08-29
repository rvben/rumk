use crate::diagnostic::{Diagnostic, Edit};

pub fn apply_fixes(content: &str, diagnostics: &[Diagnostic]) -> String {
    let mut edits: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.fixable)
        .filter_map(|diagnostic| diagnostic.fix.as_ref())
        .flat_map(|fix| &fix.edits)
        .filter_map(|edit| resolve_edit(content, edit))
        .collect();

    edits.sort_by(|left, right| {
        right
            .start
            .cmp(&left.start)
            .then_with(|| right.end.cmp(&left.end))
    });

    let mut fixed = content.to_string();
    let mut next_available_offset = content.len();
    for edit in edits {
        if edit.end > next_available_offset {
            continue;
        }

        fixed.replace_range(edit.start..edit.end, edit.replacement);
        next_available_offset = edit.start;
    }

    fixed
}

struct ResolvedEdit<'a> {
    start: usize,
    end: usize,
    replacement: &'a str,
}

fn resolve_edit<'a>(content: &str, edit: &'a Edit) -> Option<ResolvedEdit<'a>> {
    let start = position_to_offset(content, edit.start_line, edit.start_column)?;
    let end = position_to_offset(content, edit.end_line, edit.end_column)?;

    (start <= end && content.is_char_boundary(start) && content.is_char_boundary(end)).then_some(
        ResolvedEdit {
            start,
            end,
            replacement: &edit.replacement,
        },
    )
}

pub fn edit_byte_range(content: &str, edit: &Edit) -> Option<(usize, usize)> {
    resolve_edit(content, edit).map(|edit| (edit.start, edit.end))
}

fn position_to_offset(content: &str, line: usize, column: usize) -> Option<usize> {
    if line == 0 || column == 0 {
        return None;
    }

    let mut line_start = 0;
    for _ in 1..line {
        let newline = content[line_start..].find('\n')?;
        line_start += newline + 1;
    }

    let mut line_end = content[line_start..]
        .find('\n')
        .map_or(content.len(), |newline| line_start + newline);
    if line_end > line_start && content.as_bytes()[line_end - 1] == b'\r' {
        line_end -= 1;
    }

    let line_content = &content[line_start..line_end];
    let byte_column = line_content
        .char_indices()
        .nth(column - 1)
        .map_or(line_content.len(), |(offset, _)| offset);
    Some(line_start + byte_column)
}
