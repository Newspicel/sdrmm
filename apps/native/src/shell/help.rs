pub const DOCS: &str = "https://sdrmm.com/docs";
pub const DISCORD: &str = "https://discord.gg/dYaRyGwBNw";

#[must_use]
pub fn docs_page(path: &str) -> String {
    format!("{DOCS}/{path}")
}

#[must_use]
pub fn api_docs(base: &str) -> String {
    format!("{}/api/docs", base.trim_end_matches('/'))
}

#[must_use]
pub fn releases(repository: &str) -> String {
    if repository.is_empty() {
        return String::new();
    }
    format!("{}/releases", repository.trim_end_matches('/'))
}

#[must_use]
pub fn host(link: &str) -> String {
    let Ok(parsed) = url::Url::parse(link) else {
        return String::new();
    };
    match (parsed.host_str(), parsed.port()) {
        (Some(name), Some(port)) => format!("{name}:{port}"),
        (Some(name), None) => name.to_owned(),
        (None, _) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::hotkeys::{BINDINGS, grouped, key_caps};

    #[test]
    fn names_where_a_link_leads() {
        assert_eq!(host(DISCORD), "discord.gg");
        assert_eq!(host(&docs_page("troubleshooting")), "sdrmm.com");
        assert_eq!(host("http://127.0.0.1:8080/api/docs"), "127.0.0.1:8080");
        assert_eq!(host(""), "");
    }

    #[test]
    fn builds_docs_api_and_release_links() {
        assert_eq!(docs_page(""), "https://sdrmm.com/docs/");
        assert_eq!(
            docs_page("user-guide/keyboard"),
            "https://sdrmm.com/docs/user-guide/keyboard"
        );
        assert_eq!(
            api_docs("http://127.0.0.1:8080/"),
            "http://127.0.0.1:8080/api/docs"
        );
        assert_eq!(
            releases("https://github.com/a/b"),
            "https://github.com/a/b/releases"
        );
        assert_eq!(releases(""), "");
    }

    #[test]
    fn a_range_dash_is_no_key_cap() {
        let caps: Vec<bool> = key_caps("1 \u{2013} 9", false)
            .into_iter()
            .map(|(_, cap)| cap)
            .collect();
        assert_eq!(caps, [true, false, true]);
    }

    #[test]
    fn lists_every_binding_exactly_once() {
        let listed: Vec<&str> = grouped(BINDINGS)
            .into_iter()
            .flat_map(|(_, bindings)| bindings.into_iter().map(|binding| binding.keys))
            .collect();
        assert_eq!(listed.len(), BINDINGS.len());
        let unique: std::collections::BTreeSet<&str> = listed.into_iter().collect();
        assert_eq!(unique.len(), BINDINGS.len());
    }
}
