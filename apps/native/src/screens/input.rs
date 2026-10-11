use std::time::Duration;

use zgui::{
    geom::{CssPx, Point, Size},
    platform::SurfaceEvent,
    vocab::{
        KeyEvent, KeyState, Modifiers, NamedKey, PhysicalKey, PointerAction, PointerButton,
        PointerEvent, PointerId, PointerKind, ScrollDelta, ScrollPhase, Timestamp, WheelEvent,
    },
};

use super::driver::Screen;

const TICK: Duration = Duration::from_millis(16);
const DRAG_STEPS: u16 = 8;

pub type At = (f32, f32);

impl Screen {
    fn tick(&mut self) -> Timestamp {
        self.clock += TICK;
        Timestamp::from_origin(self.clock)
    }

    fn send(&mut self, event: SurfaceEvent) {
        self.harness().deliver_to_first(event);
        self.pump();
    }

    pub fn pointer(
        &mut self,
        action: PointerAction,
        at: At,
        button: Option<PointerButton>,
        modifiers: Modifiers,
    ) {
        let mut event = PointerEvent::mouse(Point::new(CssPx(at.0), CssPx(at.1)));
        if let Some(button) = button {
            event = event.with_button(button);
        }
        let timestamp = self.tick();
        self.send(SurfaceEvent::Pointer {
            action,
            event,
            modifiers,
            timestamp,
        });
    }

    pub fn press(&mut self, at: At, button: PointerButton, modifiers: Modifiers) {
        self.pointer(PointerAction::Moved, at, None, modifiers);
        self.pointer(PointerAction::Pressed, at, Some(button), modifiers);
        self.pointer(PointerAction::Released, at, Some(button), modifiers);
    }

    pub fn click(&mut self, at: At) {
        self.click_with(at, Modifiers::NONE);
    }

    pub fn click_with(&mut self, at: At, modifiers: Modifiers) {
        self.press(at, PointerButton::Primary, modifiers);
        self.frame();
    }

    pub fn double_click(&mut self, at: At) {
        self.press(at, PointerButton::Primary, Modifiers::NONE);
        self.press(at, PointerButton::Primary, Modifiers::NONE);
        self.frame();
    }

    pub fn right_click(&mut self, at: At) {
        self.press(at, PointerButton::Secondary, Modifiers::NONE);
        self.frame();
    }

    pub fn click_on(&mut self, selector: &str) {
        let at = self.center_of(selector);
        self.click(at);
    }

    pub fn drag(&mut self, from: At, to: At, modifiers: Modifiers) {
        self.drag_with(from, to, PointerButton::Primary, modifiers);
    }

    pub fn drag_with(&mut self, from: At, to: At, button: PointerButton, modifiers: Modifiers) {
        self.pointer(PointerAction::Moved, from, None, modifiers);
        self.pointer(PointerAction::Pressed, from, Some(button), modifiers);
        for step in 1..=DRAG_STEPS {
            let t = f32::from(step) / f32::from(DRAG_STEPS);
            let at = (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
            self.pointer(PointerAction::Moved, at, None, modifiers);
            self.frame();
        }
        self.pointer(PointerAction::Released, to, Some(button), modifiers);
        self.frame();
    }

    pub fn wheel(&mut self, at: At, delta: At, modifiers: Modifiers) {
        let timestamp = self.tick();
        let event = WheelEvent {
            delta: ScrollDelta::Pixels(Size::new(CssPx(delta.0), CssPx(delta.1))),
            phase: ScrollPhase::Discrete,
            position: Point::new(CssPx(at.0), CssPx(at.1)),
            id: PointerId::new(0),
            kind: PointerKind::Mouse,
        };
        self.send(SurfaceEvent::Wheel {
            event,
            modifiers,
            timestamp,
        });
        self.frame();
    }

    fn stroke(&mut self, event: &KeyEvent, modifiers: Modifiers) {
        if !modifiers.is_empty() {
            self.send(SurfaceEvent::ModifiersChanged(modifiers));
        }
        for state in [KeyState::Pressed, KeyState::Released] {
            let timestamp = self.tick();
            self.send(SurfaceEvent::Key {
                state,
                event: event.clone(),
                modifiers,
                timestamp,
            });
        }
        if !modifiers.is_empty() {
            self.send(SurfaceEvent::ModifiersChanged(Modifiers::NONE));
        }
        self.frame();
    }

    pub fn key(&mut self, key: NamedKey, modifiers: Modifiers) {
        self.stroke(
            &KeyEvent::named(key, PhysicalKey::Unidentified(0)),
            modifiers,
        );
    }

    pub fn letter(&mut self, text: &str, modifiers: Modifiers) {
        self.stroke(&KeyEvent::character(text.to_owned()), modifiers);
    }

    pub fn type_text(&mut self, text: &str) {
        for letter in text.chars() {
            self.letter(&letter.to_string(), Modifiers::NONE);
        }
    }
}
