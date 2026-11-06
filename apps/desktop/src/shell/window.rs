use std::cell::RefCell;

use cef::*;

use super::{Start, client};

const TITLE: &str = "SDRmm";
const ICON: &[u8] = include_bytes!("../../icons/128x128.png");
const SIZE: Size = Size {
    width: 1280,
    height: 800,
};

thread_local! {
    /// The one browser, owned by the UI thread that created it.
    static BROWSER: RefCell<Option<Browser>> = const { RefCell::new(None) };
}

pub fn open(start: &Start) {
    let mut client = Some(client::new(start));
    let url = CefString::from(start.url.as_str());
    let browser_view = browser_view_create(
        client.as_mut(),
        Some(&url),
        Some(&BrowserSettings::default()),
        None,
        None,
        None::<&mut BrowserViewDelegate>,
    );
    let mut delegate = ShellWindow::new(RefCell::new(browser_view));
    window_create_top_level(Some(&mut delegate));
}

pub fn created(browser: Option<&mut Browser>) {
    BROWSER.with_borrow_mut(|slot| *slot = browser.cloned());
}

pub fn closed() {
    BROWSER.with_borrow_mut(|slot| *slot = None);
    quit_message_loop();
}

/// Closes the window from any thread.
pub fn close() {
    let mut task = CloseWindow::new();
    post_task(ThreadId::UI, Some(&mut task));
}

fn icon() -> Option<Image> {
    let image = image_create()?;
    (image.add_png(1.0, Some(ICON)) == 1).then_some(image)
}

wrap_task! {
    struct CloseWindow;

    impl Task {
        fn execute(&self) {
            let host = BROWSER.with_borrow(|browser| browser.as_ref().and_then(Browser::host));
            match host {
                Some(host) => host.close_browser(1),
                None => quit_message_loop(),
            }
        }
    }
}

wrap_window_delegate! {
    struct ShellWindow {
        browser_view: RefCell<Option<BrowserView>>,
    }

    impl ViewDelegate {
        fn preferred_size(&self, _view: Option<&mut View>) -> Size {
            SIZE
        }
    }

    impl PanelDelegate {}

    impl WindowDelegate {
        fn on_window_created(&self, window: Option<&mut Window>) {
            let browser_view = self.browser_view.borrow();
            let (Some(window), Some(browser_view)) = (window, browser_view.as_ref()) else {
                return;
            };
            window.add_child_view(Some(&mut View::from(browser_view)));
            window.set_title(Some(&CefString::from(TITLE)));
            if let Some(mut icon) = icon() {
                window.set_window_app_icon(Some(&mut icon));
            }
            window.show();
        }

        fn on_window_destroyed(&self, _window: Option<&mut Window>) {
            self.browser_view.borrow_mut().take();
        }

        fn can_close(&self, _window: Option<&mut Window>) -> i32 {
            let browser = self.browser_view.borrow().as_ref().and_then(BrowserView::browser);
            match browser.and_then(|browser| browser.host()) {
                Some(host) => host.try_close_browser(),
                None => 1,
            }
        }
    }
}
