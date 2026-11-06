use std::{
    collections::HashSet,
    sync::{Arc, Mutex, PoisonError},
};

use cef::*;
use tokio::runtime::Handle;
use url::Url;

use super::{Start, window};
use crate::{
    browser::{self, Route},
    dialog,
};

pub fn new(start: &Start) -> Client {
    ShellClient::new(
        start.url.clone(),
        start.runtime.clone(),
        Arc::new(Mutex::new(HashSet::new())),
    )
}

wrap_client! {
    struct ShellClient {
        app: Url,
        runtime: Handle,
        failed: Arc<Mutex<HashSet<u32>>>,
    }

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(ShellLifeSpan::new())
        }

        fn request_handler(&self) -> Option<RequestHandler> {
            Some(ShellRequests::new(self.app.clone()))
        }

        fn download_handler(&self) -> Option<DownloadHandler> {
            Some(ShellDownloads::new(self.runtime.clone(), self.failed.clone()))
        }
    }
}

wrap_life_span_handler! {
    struct ShellLifeSpan;

    impl LifeSpanHandler {
        fn on_before_popup(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: i32,
            target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            _target_disposition: WindowOpenDisposition,
            _user_gesture: i32,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut i32>,
        ) -> i32 {
            if let Some(url) = target_url {
                browser::open(&url.to_string());
            }
            1
        }

        fn on_after_created(&self, browser: Option<&mut Browser>) {
            window::created(browser);
        }

        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            window::closed();
        }
    }
}

wrap_request_handler! {
    struct ShellRequests {
        app: Url,
    }

    impl RequestHandler {
        fn on_before_browse(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _user_gesture: i32,
            _is_redirect: i32,
        ) -> i32 {
            let Some(request) = request else {
                return 0;
            };
            let url = CefString::from(&request.url()).to_string();
            match browser::route(&url, &self.app) {
                Route::Window => 0,
                Route::Browser => {
                    browser::open(&url);
                    1
                }
                Route::Refuse => {
                    tracing::warn!("refused to load {url}");
                    1
                }
            }
        }

        fn on_open_urlfrom_tab(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            target_url: Option<&CefString>,
            _target_disposition: WindowOpenDisposition,
            _user_gesture: i32,
        ) -> i32 {
            if let Some(url) = target_url {
                browser::open(&url.to_string());
            }
            1
        }
    }
}

wrap_download_handler! {
    struct ShellDownloads {
        runtime: Handle,
        failed: Arc<Mutex<HashSet<u32>>>,
    }

    impl DownloadHandler {
        fn can_download(
            &self,
            _browser: Option<&mut Browser>,
            _url: Option<&CefString>,
            _request_method: Option<&CefString>,
        ) -> i32 {
            1
        }

        fn on_before_download(
            &self,
            _browser: Option<&mut Browser>,
            _download_item: Option<&mut DownloadItem>,
            suggested_name: Option<&CefString>,
            callback: Option<&mut BeforeDownloadCallback>,
        ) -> i32 {
            let Some(callback) = callback else {
                return 0;
            };
            let name = suggested_name.map(CefString::to_string).unwrap_or_default();
            let suggested = dirs::download_dir().unwrap_or_default().join(name);
            callback.cont(Some(&CefString::from(suggested.to_string_lossy().as_ref())), 1);
            1
        }

        fn on_download_updated(
            &self,
            _browser: Option<&mut Browser>,
            download_item: Option<&mut DownloadItem>,
            _callback: Option<&mut DownloadItemCallback>,
        ) {
            let Some(item) = download_item.filter(|item| item.is_interrupted() == 1) else {
                return;
            };
            let first = self
                .failed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(item.id());
            if first {
                let name = CefString::from(&item.suggested_file_name()).to_string();
                tracing::warn!("download of {name} failed");
                self.runtime.spawn(dialog::tell(format!("Could not download {name}")));
            }
        }
    }
}
