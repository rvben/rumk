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

/// Applies non-overlapping fixes, preferring the fix furthest into the text.
/// Each fix is accepted or rejected as a whole. Rejected fixes are omitted
/// from [`Applied::fixed`] and can be offered again after re-linting.
///
/// Edits are resolved against the original text. Unchanged slices and
/// replacements are collected backwards, then written once in source order.
pub fn apply_fixes(content: &str, diagnostics: &[Diagnostic]) -> Applied {
    let mut positions = None;
    let mut fixes: Vec<_> = diagnostics
        .iter()
        .enumerate()
        .filter(|(_, diagnostic)| diagnostic.fixable)
        .filter(|(_, diagnostic)| diagnostic.fix.is_some())
        .filter_map(|(index, diagnostic)| {
            let positions = positions.get_or_insert_with(|| LineIndex::new(content));
            Some((index, resolve_fix(positions, diagnostic)?))
        })
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
    let mut chunks = Vec::new();
    let mut cursor = content.len();
    let mut next_available_offset = content.len();
    for (index, fix) in fixes {
        if fix.end > next_available_offset {
            continue;
        }

        for edit in &fix.edits {
            chunks.push(&content[edit.end..cursor]);
            chunks.push(edit.replacement);
            cursor = edit.start;
        }
        next_available_offset = fix.start;
        applied.push(index);
    }

    chunks.push(&content[..cursor]);
    let mut fixed = String::with_capacity(chunks.iter().map(|chunk| chunk.len()).sum());
    for chunk in chunks.into_iter().rev() {
        fixed.push_str(chunk);
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
fn resolve_fix<'a>(
    positions: &LineIndex<'_>,
    diagnostic: &'a Diagnostic,
) -> Option<ResolvedFix<'a>> {
    let mut edits = diagnostic
        .fix
        .as_ref()?
        .edits
        .iter()
        .map(|edit| resolve_edit(positions, edit))
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

fn resolve_edit<'a>(positions: &LineIndex<'_>, edit: &'a Edit) -> Option<ResolvedEdit<'a>> {
    let content = positions.content;
    let start = positions.offset(edit.start_line, edit.start_column)?;
    let end = positions.offset(edit.end_line, edit.end_column)?;

    (start <= end && content.is_char_boundary(start) && content.is_char_boundary(end)).then_some(
        ResolvedEdit {
            start,
            end,
            replacement: &edit.replacement,
        },
    )
}

pub fn edit_byte_range(content: &str, edit: &Edit) -> Option<(usize, usize)> {
    resolve_edit(&LineIndex::new(content), edit).map(|edit| (edit.start, edit.end))
}

struct LineIndex<'a> {
    content: &'a str,
    starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    fn new(content: &'a str) -> Self {
        let mut starts = vec![0];
        starts.extend(content.match_indices('\n').map(|(offset, _)| offset + 1));
        Self { content, starts }
    }

    fn offset(&self, line: usize, column: usize) -> Option<usize> {
        let content = self.content;
        if line == 0 || column == 0 {
            return None;
        }

        let line_start = *self.starts.get(line - 1)?;
        let mut line_end = self.starts.get(line).map_or(content.len(), |next| next - 1);
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
}
