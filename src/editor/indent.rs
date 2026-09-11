use std::ops::Range;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum IndentDirection {
    Increase,
    Decrease,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct IndentEdit {
    pub range: Range<usize>,
    pub replacement: String,
}

pub(super) fn indent_lines(
    text: &str,
    first_line: usize,
    last_line: usize,
    direction: IndentDirection,
    width: usize,
) -> (String, Vec<IndentEdit>) {
    let mut output = String::with_capacity(text.len());
    let mut edits = Vec::new();
    for (line_number, line) in text.split_inclusive('\n').enumerate() {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let leading = content
            .bytes()
            .take_while(|byte| matches!(byte, b' ' | b'\t'))
            .count();
        let line_start = text
            .split_inclusive('\n')
            .take(line_number)
            .map(str::len)
            .sum();
        let replacement =
            if (first_line..=last_line).contains(&line_number) && !content[leading..].is_empty() {
                match direction {
                    IndentDirection::Increase => " ".repeat(leading.saturating_add(width)),
                    IndentDirection::Decrease => " ".repeat(leading.saturating_sub(width)),
                }
            } else {
                content[..leading].to_owned()
            };
        if replacement.len() != leading {
            edits.push(IndentEdit {
                range: line_start..line_start + leading,
                replacement: replacement.clone(),
            });
        }
        output.push_str(&replacement);
        output.push_str(&content[leading..]);
        if line.ends_with('\n') {
            output.push('\n');
        }
    }
    (output, edits)
}

#[cfg(test)]
mod tests {
    use super::{IndentDirection, indent_lines};

    #[test]
    fn increases_and_decreases_only_leading_whitespace() {
        let (indented, _) =
            indent_lines("select 1;\n  select 2;", 0, 1, IndentDirection::Increase, 4);
        assert_eq!(indented, "    select 1;\n      select 2;");

        let (unindented, _) = indent_lines(&indented, 0, 1, IndentDirection::Decrease, 4);
        assert_eq!(unindented, "select 1;\n  select 2;");
    }

    #[test]
    fn skips_empty_lines_and_clamps_decrease() {
        let (output, _) = indent_lines("\n  \n  select 1;", 0, 2, IndentDirection::Decrease, 4);
        assert_eq!(output, "\n  \nselect 1;");
    }

    #[test]
    fn handles_tabs_as_leading_whitespace() {
        let (output, _) = indent_lines("\tselect 1;", 0, 0, IndentDirection::Increase, 4);
        assert_eq!(output, "     select 1;");
    }
}
