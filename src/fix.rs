use crate::diagnostic::Diagnostic;

pub fn apply_fixes(content: &str, diagnostics: &[Diagnostic]) -> String {
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let mut fixable_diagnostics: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.fixable && d.fix.is_some())
        .collect();
    
    fixable_diagnostics.sort_by(|a, b| {
        b.line.cmp(&a.line)
            .then_with(|| b.column.cmp(&a.column))
    });
    
    for diagnostic in fixable_diagnostics {
        if let Some(fix) = &diagnostic.fix {
            for edit in &fix.edits {
                apply_edit(&mut lines, edit);
            }
        }
    }
    
    lines.join("\n")
}

fn apply_edit(lines: &mut Vec<String>, edit: &crate::diagnostic::Edit) {
    if edit.start_line == 0 || edit.start_line > lines.len() {
        return;
    }
    
    let line_idx = edit.start_line - 1;
    
    if edit.start_line == edit.end_line {
        if let Some(line) = lines.get_mut(line_idx) {
            let start_col = edit.start_column.saturating_sub(1);
            let end_col = edit.end_column.saturating_sub(1).min(line.len());
            
            if start_col <= line.len() && start_col <= end_col {
                line.replace_range(start_col..end_col, &edit.replacement);
            }
        }
    } else {
        let start_col = edit.start_column.saturating_sub(1);
        let end_line_idx = (edit.end_line - 1).min(lines.len() - 1);
        let end_col = edit.end_column.saturating_sub(1);
        
        if let Some(first_line) = lines.get_mut(line_idx) {
            let prefix = first_line[..start_col.min(first_line.len())].to_string();
            let suffix = if end_line_idx < lines.len() {
                lines[end_line_idx][end_col.min(lines[end_line_idx].len())..].to_string()
            } else {
                String::new()
            };
            
            *first_line = format!("{}{}{}", prefix, edit.replacement, suffix);
            
            for _ in line_idx + 1..=end_line_idx.min(lines.len() - 1) {
                if line_idx + 1 < lines.len() {
                    lines.remove(line_idx + 1);
                }
            }
        }
    }
}