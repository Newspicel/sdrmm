use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use syn::visit::{self, Visit};

const MAX_FILE_LINES: usize = 3000;
const MAX_FUNCTION_LINES: usize = 200;
const RUST_TREES: [&str; 4] = ["crates", "apps", "xtask", "fuzz"];
const SCRIPT_TREES: [&str; 2] = ["web/src", "site/src"];

#[derive(Debug, PartialEq, Eq)]
struct Oversize {
    path: PathBuf,
    line: usize,
    what: String,
    lines: usize,
    limit: usize,
}

impl Oversize {
    fn render(&self, annotate: bool) -> String {
        let message = format!(
            "{} has {} lines, aim for at most {}",
            self.what, self.lines, self.limit
        );
        let path = self.path.display();
        if annotate {
            format!("::warning file={path},line={}::{message}", self.line)
        } else {
            format!("warning: {path}:{}: {message}", self.line)
        }
    }
}

/// Soft size gate: oversized files and functions are reported, never fatal.
pub(crate) fn check(root: &Path) -> Result<()> {
    let annotate = std::env::var_os("GITHUB_ACTIONS").is_some();
    for oversize in scan(root)? {
        println!("{}", oversize.render(annotate));
    }
    Ok(())
}

fn scan(root: &Path) -> Result<Vec<Oversize>> {
    let mut found = Vec::new();
    for tree in RUST_TREES {
        for path in crate::sources::files(&root.join(tree), &["rs"])? {
            let text = read(&path)?;
            let relative = relative(root, &path);
            found.extend(oversized_file(&relative, &text));
            found.extend(rust_functions(&relative, &text));
        }
    }
    for tree in SCRIPT_TREES {
        for path in crate::sources::files(&root.join(tree), &["ts", "tsx"])? {
            if path.to_string_lossy().ends_with(".d.ts") {
                continue;
            }
            let text = read(&path)?;
            let relative = relative(root, &path);
            found.extend(oversized_file(&relative, &text));
            found.extend(script_functions(&relative, &text));
        }
    }
    Ok(found)
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))
}

fn relative(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

fn oversized_file(path: &Path, text: &str) -> Option<Oversize> {
    let lines = text.lines().count();
    (lines > MAX_FILE_LINES).then(|| Oversize {
        path: path.to_path_buf(),
        line: 1,
        what: "file".into(),
        lines,
        limit: MAX_FILE_LINES,
    })
}

fn function(path: &Path, name: &str, start: usize, end: usize) -> Option<Oversize> {
    let lines = end + 1 - start;
    (lines > MAX_FUNCTION_LINES).then(|| Oversize {
        path: path.to_path_buf(),
        line: start,
        what: format!("`{name}`"),
        lines,
        limit: MAX_FUNCTION_LINES,
    })
}

fn rust_functions(path: &Path, text: &str) -> Vec<Oversize> {
    let file = match syn::parse_file(text) {
        Ok(file) => file,
        Err(error) => {
            println!(
                "warning: {}: not measured, syn cannot parse it: {error}",
                path.display()
            );
            return Vec::new();
        }
    };
    let mut visitor = RustFunctions {
        path,
        found: Vec::new(),
    };
    visitor.visit_file(&file);
    visitor.found
}

struct RustFunctions<'a> {
    path: &'a Path,
    found: Vec<Oversize>,
}

impl RustFunctions<'_> {
    fn measure(&mut self, sig: &syn::Signature, block: &syn::Block) {
        let start = sig.fn_token.span.start().line;
        let end = block.brace_token.span.close().end().line;
        self.found
            .extend(function(self.path, &sig.ident.to_string(), start, end));
    }
}

impl<'ast> Visit<'ast> for RustFunctions<'_> {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.measure(&item.sig, &item.block);
        visit::visit_item_fn(self, item);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.measure(&item.sig, &item.block);
        visit::visit_impl_item_fn(self, item);
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        if let Some(block) = &item.default {
            self.measure(&item.sig, block);
        }
        visit::visit_trait_item_fn(self, item);
    }
}

/// Measures top-level functions and components. Biome formats every one of them to open on an
/// unindented line ending in `{` and close on the next unindented line of bare closing brackets,
/// so no TypeScript parser is needed.
fn script_functions(path: &Path, text: &str) -> Vec<Oversize> {
    let mut found = Vec::new();
    let mut open: Option<(usize, String)> = None;
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        match &open {
            None => open = script_function_start(line).map(|name| (number, name)),
            Some((start, name)) if closes_top_level(line) => {
                found.extend(function(path, name, *start, number));
                open = None;
            }
            Some(_) => {}
        }
    }
    found
}

fn script_function_start(line: &str) -> Option<String> {
    if !line.trim_end().ends_with(['{', '(']) {
        return None;
    }
    let mut rest = line;
    for keyword in ["export ", "default ", "async "] {
        rest = rest.strip_prefix(keyword).unwrap_or(rest);
    }
    if let Some(after) = rest.strip_prefix("function") {
        let name = identifier(after.trim_start_matches(['*', ' ']));
        return Some(if name.is_empty() {
            "default".into()
        } else {
            name
        });
    }
    let after = rest
        .strip_prefix("const ")
        .or_else(|| rest.strip_prefix("let "))?;
    let name = identifier(after);
    let (_, value) = after.split_once(" = ")?;
    let value = value.strip_prefix("async ").unwrap_or(value);
    let callable = value.starts_with(['(', '<'])
        || value.starts_with("function")
        || value.starts_with("memo(")
        || value.starts_with("forwardRef(")
        || value.contains("=> {");
    (callable && !name.is_empty()).then_some(name)
}

fn identifier(text: &str) -> String {
    text.chars()
        .take_while(|char| char.is_alphanumeric() || *char == '_' || *char == '$')
        .collect()
}

fn closes_top_level(line: &str) -> bool {
    let line = line.trim_end();
    !line.is_empty()
        && line
            .chars()
            .all(|char| matches!(char, '}' | ')' | ']' | ';' | ','))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(lines: usize) -> String {
        (0..lines)
            .map(|line| format!("    let _x{line} = {line};\n"))
            .collect()
    }

    #[test]
    fn a_long_rust_function_is_reported_at_its_fn_keyword() {
        let text = format!(
            "/// Docs.\nfn short() {{}}\n\nimpl A {{\n    fn long(&self) {{\n{}    }}\n}}\n",
            body(199)
        );
        let found = rust_functions(Path::new("a.rs"), &text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 5);
        assert_eq!(found[0].lines, 201);
        assert_eq!(found[0].what, "`long`");
    }

    #[test]
    fn a_rust_function_at_the_limit_passes() {
        let text = format!("fn fits() {{\n{}}}\n", body(198));
        assert!(rust_functions(Path::new("a.rs"), &text).is_empty());
    }

    #[test]
    fn trait_defaults_count_and_declarations_do_not() {
        let text = format!(
            "trait T {{\n    fn declared(&self);\n    fn provided(&self) {{\n{}    }}\n}}\n",
            body(250)
        );
        let found = rust_functions(Path::new("a.rs"), &text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].what, "`provided`");
    }

    #[test]
    fn unparsable_rust_is_skipped() {
        assert!(rust_functions(Path::new("a.rs"), "fn (").is_empty());
    }

    #[test]
    fn a_long_file_is_reported() {
        let text = "x\n".repeat(3001);
        let found = oversized_file(Path::new("a.rs"), &text);
        assert_eq!(found.map(|oversize| oversize.lines), Some(3001));
        assert!(oversized_file(Path::new("a.rs"), &"x\n".repeat(3000)).is_none());
    }

    fn component(head: &str, tail: &str, lines: usize) -> String {
        let jsx: String = (0..lines).map(|_| "  <p>Don't {\"}\"}</p>\n").collect();
        format!("{head}\n{jsx}{tail}\n")
    }

    #[test]
    fn script_functions_measure_every_top_level_form() {
        let text = [
            component("export function Panel({", "}: Props) {", 0),
            component("", "}", 200),
            component("export const Face = memo(function Face() {", "});", 120),
            component("const helper = async (a: number) => {", "};", 210),
            component("export default function () {", "}", 50),
        ]
        .concat();
        let found = script_functions(Path::new("a.tsx"), &text);
        let named: Vec<(&str, usize)> = found
            .iter()
            .map(|oversize| (oversize.what.as_str(), oversize.lines))
            .collect();
        assert_eq!(named, [("`Panel`", 204), ("`helper`", 212)]);
    }

    #[test]
    fn objects_and_types_are_not_functions() {
        let text = component("export const table = {", "};", 300)
            + &component("type Props = {", "};", 300)
            + &component("export const f = (a: number) =>", "  a;", 300);
        assert!(script_functions(Path::new("a.ts"), &text).is_empty());
    }

    #[test]
    fn warnings_become_github_annotations_on_actions() {
        let oversize = Oversize {
            path: PathBuf::from("web/src/A.tsx"),
            line: 7,
            what: "`A`".into(),
            lines: 250,
            limit: 200,
        };
        assert_eq!(
            oversize.render(true),
            "::warning file=web/src/A.tsx,line=7::`A` has 250 lines, aim for at most 200"
        );
        assert_eq!(
            oversize.render(false),
            "warning: web/src/A.tsx:7: `A` has 250 lines, aim for at most 200"
        );
    }
}
