#[derive(Debug, Default, PartialEq, Eq)]
pub struct Compound {
    classes: Vec<String>,
    attrs: Vec<(String, Option<String>)>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Selector(pub Vec<Compound>);

pub trait Element {
    fn classes(&self) -> Vec<String>;
    fn attr(&self, name: &str) -> Option<String>;
    fn text(&self) -> String;
}

impl Selector {
    pub fn parse(text: &str) -> Self {
        Self(words(text).iter().map(|word| compound(word)).collect())
    }
}

impl Compound {
    pub fn matches(&self, element: &dyn Element) -> bool {
        let classes = element.classes();
        if !self.classes.iter().all(|wanted| classes.contains(wanted)) {
            return false;
        }
        self.attrs.iter().all(|(name, value)| match name.as_str() {
            "text" => value
                .as_ref()
                .is_some_and(|value| element.text().contains(value.as_str())),
            _ => match (element.attr(name), value) {
                (Some(held), Some(value)) => held == *value,
                (Some(_), None) => true,
                (None, _) => false,
            },
        })
    }
}

fn words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut depth = 0_u32;
    for letter in text.chars() {
        match letter {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if letter.is_whitespace() && depth == 0 {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
        } else {
            word.push(letter);
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

fn compound(word: &str) -> Compound {
    let mut out = Compound::default();
    let mut rest = word;
    while !rest.is_empty() {
        if let Some(inside) = rest.strip_prefix('[') {
            let end = inside.find(']').unwrap_or(inside.len());
            out.attrs.push(attribute(&inside[..end]));
            rest = inside.get(end + 1..).unwrap_or("");
        } else {
            let body = rest.trim_start_matches('.');
            let end = body.find(['.', '[']).unwrap_or(body.len());
            out.classes.push(body[..end].to_owned());
            rest = &body[end..];
        }
    }
    out
}

fn attribute(inside: &str) -> (String, Option<String>) {
    match inside.split_once('=') {
        Some((name, value)) => (name.to_owned(), Some(value.trim_matches('"').to_owned())),
        None => (inside.to_owned(), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake;

    impl Element for Fake {
        fn classes(&self) -> Vec<String> {
            vec!["zf-handle".into(), "iq".into()]
        }

        fn attr(&self, name: &str) -> Option<String> {
            (name == "data-kind").then(|| "source".into())
        }

        fn text(&self) -> String {
            "Create a workspace".into()
        }
    }

    #[test]
    fn a_selector_splits_into_compounds_outside_brackets() {
        let parsed = Selector::parse(r#".zf-node[data-id=a b] .zf-handle.iq[text="Create a"]"#);
        assert_eq!(parsed.0.len(), 2);
        assert!(parsed.0[1].matches(&Fake));
        assert!(!parsed.0[0].matches(&Fake));
    }

    #[test]
    fn attributes_match_by_value_or_presence() {
        assert!(Selector::parse("[data-kind=source]").0[0].matches(&Fake));
        assert!(Selector::parse("[data-kind]").0[0].matches(&Fake));
        assert!(!Selector::parse("[data-kind=target]").0[0].matches(&Fake));
        assert!(!Selector::parse("[data-side]").0[0].matches(&Fake));
    }
}
