//! Unified line diffs of pretty-printed JSON frames for replay failures.

use serde_json::Value;

/// A line diff of `expected` and `actual`, `-` for expected-only lines and `+` for actual-only
/// lines, computed from the longest common subsequence of pretty-printed lines.
pub(super) fn json_diff(expected: &Value, actual: &Value) -> String {
    let expected = pretty(expected);
    let actual = pretty(actual);
    let left: Vec<&str> = expected.lines().collect();
    let right: Vec<&str> = actual.lines().collect();
    let mut table = vec![vec![0_usize; right.len() + 1]; left.len() + 1];
    for (row, left_line) in left.iter().enumerate().rev() {
        for (column, right_line) in right.iter().enumerate().rev() {
            table[row][column] = if left_line == right_line {
                table[row + 1][column + 1] + 1
            } else {
                table[row + 1][column].max(table[row][column + 1])
            };
        }
    }
    let mut output = String::from("--- expected\n+++ actual\n");
    let (mut row, mut column) = (0, 0);
    while row < left.len() || column < right.len() {
        if row < left.len() && column < right.len() && left[row] == right[column] {
            output.push_str(&format!("  {}\n", left[row]));
            row += 1;
            column += 1;
        } else if column < right.len()
            && (row == left.len() || table[row][column + 1] >= table[row + 1][column])
        {
            output.push_str(&format!("+ {}\n", right[column]));
            column += 1;
        } else {
            output.push_str(&format!("- {}\n", left[row]));
            row += 1;
        }
    }
    output
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}
