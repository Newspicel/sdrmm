use std::{cell::RefCell, rc::Rc};

use zgui::{prelude::*, reactive::RenderEffect};

pub fn dismissable(root: NodeRef, exclude: Option<NodeRef>, close: impl Fn() + Clone + 'static) {
    let guards: Rc<RefCell<Vec<ListenerGuard>>> = Rc::default();
    let attached = {
        let guards = Rc::clone(&guards);
        RenderEffect::new(move |_| {
            if root.get().is_none() || !guards.borrow().is_empty() {
                return;
            }
            let Some(window) = root.window_root() else {
                return;
            };
            let outside = close.clone();
            let escape = close.clone();
            let mut held = Vec::new();
            held.extend(window.listen(
                events::POINTER_DOWN,
                ListenerOptions::CAPTURE,
                move |ev: &mut EventCx<'_, events::PointerDown>| {
                    let inside = root.contains(ev.target)
                        || exclude.is_some_and(|node| node.contains(ev.target));
                    if !inside {
                        outside();
                    }
                },
            ));
            held.extend(window.listen(
                events::KEY_DOWN,
                ListenerOptions::CAPTURE,
                move |ev: &mut EventCx<'_, events::KeyDown>| {
                    if ev.key == Key::Named(NamedKey::Escape) {
                        ev.stop_immediate_propagation();
                        ev.prevent_default();
                        escape();
                    }
                },
            ));
            root.focus();
            *guards.borrow_mut() = held;
        })
    };
    on_cleanup_local(move || {
        drop(attached);
        guards.borrow_mut().clear();
    });
}

pub fn focus_when_bound(field: NodeRef) {
    let focus = RenderEffect::new(move |done: Option<bool>| {
        if done == Some(true) {
            return true;
        }
        if field.get().is_some() {
            field.focus();
            return true;
        }
        false
    });
    on_cleanup_local(move || drop(focus));
}
