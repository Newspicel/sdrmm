use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

const TREES: [&str; 4] = ["crates", "apps", "xtask", "fuzz"];

pub(crate) fn check(root: &Path) -> Result<()> {
    let mut offenders = Vec::new();
    for tree in TREES {
        let dir = root.join(tree);
        if !dir.is_dir() {
            continue;
        }
        for source in sources(&dir)? {
            let text = std::fs::read_to_string(&source)
                .with_context(|| format!("read {}", source.display()))?;
            let relative = source.strip_prefix(root).unwrap_or(&source).to_path_buf();
            offenders.extend(
                non_doc_comments(&text)
                    .into_iter()
                    .map(|line| format!("{}:{line}", relative.display())),
            );
        }
    }
    ensure!(
        offenders.is_empty(),
        "code explains itself: turn these into doc comments or names\n  {}",
        offenders.join("\n  ")
    );
    Ok(())
}

fn sources(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let path = entry
            .with_context(|| format!("read {}", dir.display()))?
            .path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            out.extend(sources(&path)?);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

fn non_doc_comments(text: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut line = 1;
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'\n' => {
                line += 1;
                at += 1;
            }
            b'"' => at = skip_quoted(bytes, at, &mut line),
            b'r' if matches!(bytes.get(at + 1), Some(b'"' | b'#')) => {
                at = skip_raw(bytes, at, &mut line).unwrap_or(at + 1);
            }
            b'\'' => at = skip_char(bytes, at),
            b'/' if bytes.get(at + 1) == Some(&b'/') => {
                let end = line_end(bytes, at);
                if !is_doc_line(&bytes[at..end]) && !is_layout_marker(&bytes[at..end]) {
                    found.push(line);
                }
                at = end;
            }
            b'/' if bytes.get(at + 1) == Some(&b'*') => {
                let end = block_end(bytes, at);
                if !matches!(bytes.get(at + 2), Some(b'*' | b'!')) {
                    found.push(line);
                }
                line += count_newlines(&bytes[at..end]);
                at = end;
            }
            _ => at += 1,
        }
    }
    found
}

fn is_doc_line(comment: &[u8]) -> bool {
    matches!(comment.get(2), Some(b'/' | b'!'))
}

fn is_layout_marker(comment: &[u8]) -> bool {
    comment[2..].iter().all(u8::is_ascii_whitespace)
}

fn line_end(bytes: &[u8], from: usize) -> usize {
    bytes[from..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(bytes.len(), |offset| from + offset)
}

fn block_end(bytes: &[u8], from: usize) -> usize {
    bytes[from + 2..]
        .windows(2)
        .position(|pair| pair == b"*/")
        .map_or(bytes.len(), |offset| from + 2 + offset + 2)
}

fn count_newlines(bytes: &[u8]) -> usize {
    bytes.iter().filter(|byte| **byte == b'\n').count()
}

fn skip_quoted(bytes: &[u8], from: usize, line: &mut usize) -> usize {
    let mut at = from + 1;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 2,
            b'"' => return at + 1,
            b'\n' => {
                *line += 1;
                at += 1;
            }
            _ => at += 1,
        }
    }
    at
}

fn skip_raw(bytes: &[u8], from: usize, line: &mut usize) -> Option<usize> {
    let mut at = from + 1;
    let mut hashes = 0;
    while bytes.get(at) == Some(&b'#') {
        hashes += 1;
        at += 1;
    }
    if bytes.get(at) != Some(&b'"') {
        return None;
    }
    at += 1;
    while at < bytes.len() {
        if bytes[at] == b'\n' {
            *line += 1;
        }
        if bytes[at] == b'"'
            && bytes[at + 1..]
                .iter()
                .take(hashes)
                .all(|byte| *byte == b'#')
        {
            return Some(at + 1 + hashes);
        }
        at += 1;
    }
    Some(at)
}

fn skip_char(bytes: &[u8], from: usize) -> usize {
    if bytes.get(from + 1) == Some(&b'\\') {
        let mut at = from + 2;
        while at < bytes.len() && bytes[at] != b'\'' {
            at += 1;
        }
        return at + 1;
    }
    if bytes.get(from + 2) == Some(&b'\'') {
        return from + 3;
    }
    from + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rust_tree_carries_no_explaining_comment() {
        check(&crate::root()).unwrap();
    }

    #[test]
    fn a_comment_after_code_is_reported() {
        assert_eq!(non_doc_comments("let a = 1; // why\n"), [1]);
    }

    #[test]
    fn doc_comments_are_left_alone() {
        assert!(non_doc_comments("/// why\n//! why\n/** why */\n/*! why */\n").is_empty());
    }

    #[test]
    fn a_bare_marker_keeps_a_tables_shape() {
        assert!(non_doc_comments("    '/', '0', //\n    '1', '2', //\n").is_empty());
    }

    #[test]
    fn a_block_comment_is_reported_at_its_opening_line() {
        assert_eq!(
            non_doc_comments("fn a() {}\n/* why\n   more */\nfn b() {}\n"),
            [2]
        );
    }

    #[test]
    fn comments_inside_literals_are_not_comments() {
        let text = concat!(
            "let a = \"x // y\";\n",
            "let b = r#\"x // y\"#;\n",
            "let c = r##\"x \"# // y\"##;\n",
            "let d = \"escaped \\\" // y\";\n",
            "let e = '/';\n",
        );
        assert!(
            non_doc_comments(text).is_empty(),
            "{:?}",
            non_doc_comments(text)
        );
    }

    #[test]
    fn a_comment_after_a_literal_is_still_found() {
        assert_eq!(non_doc_comments("let a = \"x // y\"; // real\n"), [1]);
    }
}
