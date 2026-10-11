use comfy_table::{Cell, CellAlignment, ContentLineStyle, LineStyle, Table, TableStyle};
use crossterm::style::Stylize;

// solid box, one rule under the header, no lines between rows
const STYLE: TableStyle = TableStyle::new()
    .top_border(LineStyle::new('┌', '─', '┬', '┐'))
    .header_lines(ContentLineStyle::new('│', '│', '│'))
    .header_separator(LineStyle::new('├', '─', '┼', '┤'))
    .content_lines(ContentLineStyle::new('│', '│', '│'))
    .bottom_border(LineStyle::new('└', '─', '┴', '┘'));

fn styled() -> Table {
    let mut table = Table::new();
    table.load_style(STYLE);
    table
}

/// Headerless key/value table. The section label above it does the job a
/// header row would, without spending a line on it. `numeric` right-aligns
/// the value column, which reads well for figures and badly for text.
pub fn pairs(label: &str, rows: &[(&str, String)], numeric: bool) {
    let mut table = styled();
    for (key, value) in rows {
        let value = Cell::new(value);
        table.add_row(vec![
            Cell::new(key),
            if numeric {
                value.set_alignment(CellAlignment::Right)
            } else {
                value
            },
        ]);
    }
    println!("{}", label.bold());
    println!("{table}");
}

/// Column table. First column stays left-aligned; the rest are numeric and right-aligned.
pub fn columns(label: &str, headers: &[&str], rows: Vec<Vec<String>>) {
    let mut table = styled();
    table.set_header(headers.iter().map(Cell::new).collect::<Vec<_>>());
    for row in rows {
        table.add_row(
            row.into_iter()
                .enumerate()
                .map(|(index, value)| {
                    let cell = Cell::new(value);
                    if index == 0 {
                        cell
                    } else {
                        cell.set_alignment(CellAlignment::Right)
                    }
                })
                .collect::<Vec<_>>(),
        );
    }
    println!("{}", label.bold());
    println!("{table}");
}

pub fn commas(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (index, digit) in digits.char_indices() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::commas;

    #[test]
    fn groups_digits_in_threes() {
        assert_eq!(commas(0), "0");
        assert_eq!(commas(480), "480");
        assert_eq!(commas(5760), "5,760");
        assert_eq!(commas(12000), "12,000");
        assert_eq!(commas(1234567), "1,234,567");
    }
}
