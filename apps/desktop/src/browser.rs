use url::Url;

/// Where a navigation out of the window goes.
#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    Window,
    Browser,
    Refuse,
}

/// The window shows the embedded server and the pages Chromium makes of it; other web pages
/// open in the system browser, and nothing else loads.
pub fn route(url: &str, app: &Url) -> Route {
    let Ok(parsed) = Url::parse(url) else {
        return Route::Refuse;
    };
    match parsed.scheme() {
        "http" | "https" if parsed.origin() == app.origin() => Route::Window,
        "http" | "https" => Route::Browser,
        "blob" | "data" | "about" | "chrome-error" | "devtools" => Route::Window,
        _ => Route::Refuse,
    }
}

pub fn open(url: &str) {
    let web = Url::parse(url).is_ok_and(|url| matches!(url.scheme(), "http" | "https"));
    if !web {
        tracing::warn!("refused to open {url}");
    } else if let Err(e) = opener::open_browser(url) {
        tracing::warn!("could not open {url}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> Url {
        "http://127.0.0.1:4711/".parse().unwrap()
    }

    #[test]
    fn the_embedded_server_stays_in_the_window() {
        assert_eq!(
            route("http://127.0.0.1:4711/flow?id=1", &app()),
            Route::Window
        );
        assert_eq!(
            route("blob:http://127.0.0.1:4711/f00", &app()),
            Route::Window
        );
    }

    #[test]
    fn other_web_pages_leave_for_the_browser() {
        assert_eq!(
            route("https://discord.gg/dYaRyGwBNw", &app()),
            Route::Browser
        );
        assert_eq!(route("http://127.0.0.1:4712/", &app()), Route::Browser);
    }

    #[test]
    fn local_files_and_scripts_never_load() {
        assert_eq!(route("file:///etc/passwd", &app()), Route::Refuse);
        assert_eq!(route("javascript:alert(1)", &app()), Route::Refuse);
        assert_eq!(route("not a url", &app()), Route::Refuse);
    }
}
