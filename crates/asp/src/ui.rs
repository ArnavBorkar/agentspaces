//! Terminal output helpers: minimal ANSI styling (NO_COLOR-aware), aligned
//! tables, and the JSON output contract shared by every command.

use std::io::IsTerminal;

use serde::Serialize;

pub fn color_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
}

fn style(s: &str, code: &str) -> String {
    if color_enabled() {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

pub fn bold(s: &str) -> String {
    style(s, "1")
}
pub fn dim(s: &str) -> String {
    style(s, "2")
}
pub fn green(s: &str) -> String {
    style(s, "32")
}
pub fn red(s: &str) -> String {
    style(s, "31")
}
pub fn yellow(s: &str) -> String {
    style(s, "33")
}
pub fn cyan(s: &str) -> String {
    style(s, "36")
}

/// Render rows as an aligned table. The first row is the header.
pub fn table(rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut widths = vec![0usize; cols];
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(visible_len(cell));
        }
    }
    let mut out = String::new();
    for (ri, row) in rows.iter().enumerate() {
        let mut line = String::new();
        for (i, cell) in row.iter().enumerate() {
            line.push_str(cell);
            if i + 1 < row.len() {
                let pad = widths[i].saturating_sub(visible_len(cell)) + 2;
                line.push_str(&" ".repeat(pad));
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
        if ri == 0 {
            let total: usize = widths.iter().sum::<usize>() + 2 * (cols.saturating_sub(1));
            out.push_str(&dim(&"─".repeat(total.min(100))));
            out.push('\n');
        }
    }
    out
}

/// Length excluding ANSI escape sequences.
fn visible_len(s: &str) -> usize {
    let mut len = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c == 'm' {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            len += 1;
        }
    }
    len
}

/// Success envelope for --json mode.
pub fn print_json<T: Serialize>(ok: bool, payload: &T) {
    let body = serde_json::json!({ "ok": ok, "result": payload });
    println!(
        "{}",
        serde_json::to_string_pretty(&body).expect("json encodes")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_aligns() {
        let t = table(&[
            vec!["NAME".into(), "N".into()],
            vec!["a-long-name".into(), "1".into()],
            vec!["b".into(), "22".into()],
        ]);
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines.len(), 4);
        assert!(lines[2].starts_with("a-long-name  1"));
        assert!(lines[3].starts_with("b            22"));
    }

    #[test]
    fn visible_len_ignores_ansi() {
        assert_eq!(visible_len("\x1b[32mok\x1b[0m"), 2);
    }
}
