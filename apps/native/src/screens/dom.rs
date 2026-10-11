use zgui::{
    geom::{Css, CssPx, Point, Rect, Size},
    runtime::window::Window,
    view::{NodeId, ViewHost},
};

use super::select::{Element, Selector};

pub type CssRect = Rect<CssPx, Css>;

struct Snap {
    id: Option<NodeId>,
    classes: Vec<String>,
    attrs: Vec<(String, String)>,
    text: Option<String>,
    children: Vec<usize>,
}

struct Tree(Vec<Snap>);

struct At<'a> {
    tree: &'a Tree,
    at: usize,
}

impl Element for At<'_> {
    fn classes(&self) -> Vec<String> {
        self.tree.0[self.at].classes.clone()
    }

    fn attr(&self, name: &str) -> Option<String> {
        self.tree.0[self.at]
            .attrs
            .iter()
            .find(|(held, _)| held == name)
            .map(|(_, value)| value.clone())
    }

    fn text(&self) -> String {
        let mut text = String::new();
        self.tree.gather(self.at, &mut text);
        text
    }
}

impl Tree {
    fn gather(&self, at: usize, into: &mut String) {
        if let Some(text) = &self.0[at].text {
            into.push_str(text);
        }
        for &child in &self.0[at].children {
            self.gather(child, into);
        }
    }

    fn matching(&self, selector: &Selector) -> Vec<usize> {
        let mut found = Vec::new();
        if !self.0.is_empty() && !selector.0.is_empty() {
            self.walk(0, 0, selector, &mut found);
        }
        found
    }

    fn walk(&self, at: usize, reached: usize, selector: &Selector, found: &mut Vec<usize>) {
        let last = selector.0.len() - 1;
        let element = At { tree: self, at };
        if reached == last && selector.0[last].matches(&element) {
            found.push(at);
        }
        let next = if reached < last && selector.0[reached].matches(&element) {
            reached + 1
        } else {
            reached
        };
        for &child in &self.0[at].children {
            self.walk(child, next, selector, found);
        }
    }
}

fn names(selector: &str) -> Vec<String> {
    selector
        .split('[')
        .skip(1)
        .filter_map(|part| part.split([']', '=']).next())
        .map(str::to_owned)
        .collect()
}

fn snapshot(window: &Window, names: &[String]) -> Tree {
    let document = window.document().borrow();
    let store = document.store();
    let mut tree = Tree(Vec::new());
    let Some(root) = document.root_index() else {
        return tree;
    };
    let mut stack = vec![(root, None::<usize>)];
    while let Some((index, parent)) = stack.pop() {
        let node = document.node(index);
        let key = store.key_of(index);
        let at = tree.0.len();
        tree.0.push(Snap {
            id: NodeId::from_u64(key.as_u64()),
            classes: node
                .attr_text("class")
                .map(|held| held.split_whitespace().map(str::to_owned).collect())
                .unwrap_or_default(),
            attrs: names
                .iter()
                .filter_map(|name| {
                    node.attr_text(name)
                        .map(|held| (name.clone(), held.into_owned()))
                })
                .collect(),
            text: store
                .columns()
                .text
                .get(key)
                .and_then(Option::as_deref)
                .map(str::to_owned),
            children: Vec::new(),
        });
        if let Some(parent) = parent {
            tree.0[parent].children.push(at);
        }
        let mut children = Vec::new();
        let mut child = store.core(index).first_child();
        while let Some(next) = child {
            children.push(next);
            child = store.core(next).next_sibling();
        }
        stack.extend(children.into_iter().rev().map(|child| (child, Some(at))));
    }
    tree
}

pub fn texts(window: &Window, selector: &str) -> Vec<String> {
    let parsed = Selector::parse(selector);
    let tree = snapshot(window, &names(selector));
    tree.matching(&parsed)
        .into_iter()
        .map(|at| At { tree: &tree, at }.text())
        .collect()
}

pub fn boxes(window: &Window, selector: &str) -> Vec<CssRect> {
    let parsed = Selector::parse(selector);
    let tree = snapshot(window, &names(selector));
    let scale = window.scale().get();
    tree.matching(&parsed)
        .into_iter()
        .filter_map(|at| tree.0[at].id)
        .filter_map(|id| ViewHost::window_box(window.host().as_ref(), id))
        .map(|rect| {
            Rect::new(
                Point::new(
                    CssPx(rect.origin.x.0 / scale),
                    CssPx(rect.origin.y.0 / scale),
                ),
                Size::new(
                    CssPx(rect.size.width.0 / scale),
                    CssPx(rect.size.height.0 / scale),
                ),
            )
        })
        .collect()
}

pub fn center(rect: &CssRect) -> (f32, f32) {
    (
        rect.origin.x.0 + rect.size.width.0 / 2.0,
        rect.origin.y.0 + rect.size.height.0 / 2.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_names_come_from_every_bracket() {
        assert_eq!(
            names(".zf-node[data-id=x] .zf-handle[data-kind=source][data-side]"),
            vec!["data-id", "data-kind", "data-side"]
        );
    }
}
