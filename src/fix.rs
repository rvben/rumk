use crate::diagnostic::{Diagnostic, Edit};

/// A text with the fixes that fitted into it applied.
#[derive(Debug, Clone)]
pub struct Applied {
    /// The text every applied edit has been written into.
    pub content: String,
    /// Where in the diagnostics the applied fixes came from, in the order the
    /// diagnostics were given. A fix left out of this pass is not in it.
    pub fixed: Vec<usize>,
}

/// Applies the fixes of the fixable diagnostics that carry one, working from
/// the end of the text backwards so the edits still to come keep the offsets
/// they were written against.
///
/// A fix is applied whole or not at all, and only where it reaches no further
/// than the text already rewritten. So of two fixes wanting the same span the
/// nearer the end takes it, and a fix reaching across that span waits: both are
/// left out of [`Applied::fixed`], for a caller that re-lints to offer again
/// against the rewritten text.
pub fn apply_fixes(content: &str, diagnostics: &[Diagnostic]) -> Applied {
    let mut fixes: Vec<_> = diagnostics
        .iter()
        .enumerate()
        .filter(|(_, diagnostic)| diagnostic.fixable)
        .filter_map(|(index, diagnostic)| Some((index, resolve_fix(content, diagnostic)?)))
        .collect();

    // Furthest fix first, by where it starts and then by how far it reaches, so
    // each one is compared against the span already rewritten to its right.
    fixes.sort_by(|(_, left), (_, right)| {
        right
            .start
            .cmp(&left.start)
            .then_with(|| right.end.cmp(&left.end))
    });

    let mut applied = Vec::new();
    let mut fixed = content.to_string();
    let mut next_available_offset = content.len();
    for (index, fix) in fixes {
        if fix.end > next_available_offset {
            continue;
        }

        for edit in &fix.edits {
            fixed.replace_range(edit.start..edit.end, edit.replacement);
        }
        next_available_offset = fix.start;
        applied.push(index);
    }

    applied.sort_unstable();
    Applied {
        content: fixed,
        fixed: applied,
    }
}

/// A fix's edits resolved to byte offsets, furthest into the text first.
struct ResolvedFix<'a> {
    edits: Vec<ResolvedEdit<'a>>,
    /// Where the first of its edits begins.
    start: usize,
    /// Where the last of its edits ends.
    end: usize,
}

/// Resolves the fix `diagnostic` carries, or reports `None` where it cannot be
/// applied as a whole: an edit that does not resolve against this text, or two
/// edits of the same fix wanting the same span, would leave half a fix behind.
fn resolve_fix<'a>(content: &str, diagnostic: &'a Diagnostic) -> Option<ResolvedFix<'a>> {
    let mut edits = diagnostic
        .fix
        .as_ref()?
        .edits
        .iter()
        .map(|edit| resolve_edit(content, edit))
        .collect::<Option<Vec<_>>>()?;
    edits.sort_by(|left, right| {
        right
            .start
            .cmp(&left.start)
            .then_with(|| right.end.cmp(&left.end))
    });
    if !edits.windows(2).all(|pair| pair[1].end <= pair[0].start) {
        return None;
    }

    let start = edits.last()?.start;
    let end = edits.first()?.end;
    Some(ResolvedFix { edits, start, end })
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
