use qrcodegen::{QrCode, QrCodeEcc};
use zgui::prelude::*;

use crate::ui::kit_shell::hint;

const BORDER: i32 = 2;

const SHEET: &str = css!(
    r#"
.lib-qr { width: 176px; height: 176px; padding: 6px; border-radius: 4px; background-color: #fff; flex: 0 0 auto; align-self: center; }
"#
);

#[must_use]
pub fn qr_svg(value: &str) -> Option<String> {
    let code = QrCode::encode_text(value, QrCodeEcc::Medium).ok()?;
    let side = code.size() + 2 * BORDER;
    let mut path = String::new();
    for y in 0..code.size() {
        for x in 0..code.size() {
            if code.get_module(x, y) {
                path.push_str(&format!("M{} {}h1v1h-1z", x + BORDER, y + BORDER));
            }
        }
    }
    Some(format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {side} {side}" shape-rendering="crispEdges"><rect width="{side}" height="{side}" fill="#fff"/><path d="{path}" fill="#000"/></svg>"##
    ))
}

pub fn qr_code(value: &str, label: &'static str) -> AnyView {
    install_stylesheet("library-qr", SHEET);
    match qr_svg(value) {
        Some(document) => AnyView::new(view! {
            box(a11y:role = Role::Image, a11y:label = label) {
                {zgui::elements::vector().class("lib-qr").document(&document)}
            }
        }),
        None => hint("Too long for a QR code"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_a_square_code_with_a_quiet_border() {
        let svg = qr_svg("sdrmm://pair?c=48210937").unwrap_or_default();
        assert!(svg.starts_with("<svg"));
        let side = QrCode::encode_text("sdrmm://pair?c=48210937", QrCodeEcc::Medium)
            .map(|code| code.size() + 2 * BORDER)
            .unwrap_or_default();
        assert!(svg.contains(&format!(r#"viewBox="0 0 {side} {side}""#)));
        assert!(svg.contains("M2 2h1v1h-1z"));
        assert!(!svg.contains("M0 0h1"));
    }

    #[test]
    fn gives_up_on_text_no_code_can_hold() {
        assert!(qr_svg(&"x".repeat(8_000)).is_none());
    }
}
