//! Bounded line and column selection for review comments.

use super::super::models::{ChatError, ChatResult};
use super::corrupt_data_error;
use super::patch_parser::parse_hunk_header;

const MAX_COMMENT_SELECTION_BYTES: usize = 1024 * 1024;

pub fn select_text_range(
    contents: &str,
    start_line: u64,
    start_column: u64,
    end_line: u64,
    end_column: u64,
) -> ChatResult<String> {
    let start = usize::try_from(start_line.saturating_sub(1)).map_err(|_| corrupt_data_error())?;
    let end = usize::try_from(end_line.saturating_sub(1)).map_err(|_| corrupt_data_error())?;
    if end < start {
        return Err(ChatError::validation(
            "range",
            "Review selection is outside the immutable file revision",
        ));
    }
    let full_lines = start_column == 1 && end_column == 1;
    let mut selection = String::new();
    let mut found_end = false;
    for (index, line) in contents.split('\n').enumerate() {
        if index < start {
            continue;
        }
        if index > end {
            break;
        }
        let from = if full_lines || index != start {
            0
        } else {
            column_byte_index(line, start_column)?
        };
        let to = if full_lines || index != end {
            line.len()
        } else {
            column_byte_index(line, end_column)?
        };
        if to < from {
            return Err(ChatError::validation("range", "Review range is invalid"));
        }
        if index > start {
            push_selection_text(&mut selection, "\n")?;
        }
        push_selection_text(&mut selection, &line[from..to])?;
        if index == end {
            found_end = true;
            break;
        }
    }
    if !found_end {
        return Err(ChatError::validation(
            "range",
            "Review selection is outside the immutable file revision",
        ));
    }
    Ok(selection)
}

fn column_byte_index(line: &str, column: u64) -> ChatResult<usize> {
    let target = usize::try_from(column.saturating_sub(1)).map_err(|_| corrupt_data_error())?;
    line.char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(line.len()))
        .nth(target)
        .ok_or_else(|| {
            ChatError::validation(
                "range",
                "Review column is outside the immutable file revision",
            )
        })
}

pub fn select_provider_patch_lines(
    patch: &str,
    side: &str,
    start_line: u64,
    start_column: u64,
    end_line: u64,
    end_column: u64,
) -> ChatResult<String> {
    let mut old_line = 0_u64;
    let mut new_line = 0_u64;
    let mut selected = String::new();
    let mut previous_selected_line = None;
    let full_lines = start_column == 1 && end_column == 1;
    let mut in_hunk = false;
    for line in patch.lines() {
        if line.starts_with("@@ ") {
            let (old_start, _, new_start, _) = parse_hunk_header(line)?;
            old_line = old_start;
            new_line = new_start;
            in_hunk = true;
            continue;
        }
        if !in_hunk {
            continue;
        }
        let (line_number, text) = if let Some(text) = line.strip_prefix('-') {
            let number = old_line;
            old_line = old_line.saturating_add(1);
            if side == "old" {
                (Some(number), text)
            } else {
                (None, text)
            }
        } else if let Some(text) = line.strip_prefix('+') {
            let number = new_line;
            new_line = new_line.saturating_add(1);
            if side == "new" {
                (Some(number), text)
            } else {
                (None, text)
            }
        } else if let Some(text) = line.strip_prefix(' ') {
            let number = if side == "old" { old_line } else { new_line };
            old_line = old_line.saturating_add(1);
            new_line = new_line.saturating_add(1);
            (Some(number), text)
        } else {
            continue;
        };
        if let Some(number) =
            line_number.filter(|number| *number >= start_line && *number <= end_line)
        {
            if (previous_selected_line.is_none() && number != start_line)
                || previous_selected_line.is_some_and(|previous| number != previous + 1)
            {
                return Err(ChatError::validation(
                    "range",
                    "Review selection crosses content omitted from the provider patch",
                ));
            }
            let from = if full_lines || number != start_line {
                0
            } else {
                column_byte_index(text, start_column)?
            };
            let to = if full_lines || number != end_line {
                text.len()
            } else {
                column_byte_index(text, end_column)?
            };
            if to < from {
                return Err(ChatError::validation("range", "Review range is invalid"));
            }
            if previous_selected_line.is_some() {
                push_selection_text(&mut selected, "\n")?;
            }
            push_selection_text(&mut selected, &text[from..to])?;
            previous_selected_line = Some(number);
        }
    }
    if previous_selected_line != Some(end_line) {
        return Err(ChatError::validation(
            "range",
            "Review selection is outside the provider patch",
        ));
    }
    Ok(selected)
}

fn push_selection_text(selection: &mut String, value: &str) -> ChatResult<()> {
    if selection.len().saturating_add(value.len()) > MAX_COMMENT_SELECTION_BYTES {
        return Err(ChatError::validation(
            "range",
            "Review selection exceeds the supported limit",
        ));
    }
    selection.push_str(value);
    Ok(())
}
