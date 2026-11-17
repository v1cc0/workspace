//! Linux browser hosting with an explicitly proxied WebKit network session.

use std::{
    cell::{Cell, RefCell},
    net::SocketAddr,
    rc::Rc,
};

use nsh_core::JsonValue;
use nsh_platform_linux::{ComponentContext, Components, NativeComponent, gtk4 as gtk};
use webkit6::{
    LoadEvent, NavigationPolicyDecision, NetworkProxyMode, NetworkProxySettings, NetworkSession,
    PolicyDecisionType, WebView, prelude::*,
};

/// Creates an ephemeral session whose requests use the local HTTP/CONNECT proxy.
/// The caller owns the proxy's lifetime and strict/fallback policy.
pub fn session(proxy: SocketAddr) -> NetworkSession {
    let session = NetworkSession::new_ephemeral();
    let settings = NetworkProxySettings::new(Some(&format!("http://{proxy}")), &[]);
    session.set_proxy_settings(NetworkProxyMode::Custom, Some(&settings));
    session
}

/// Registers the browser using a session configured before any page is loaded.
pub fn register(components: &Components, session: &NetworkSession) {
    let session = session.clone();
    components.register(
        "workspace.browser",
        Rc::new(move |context| Box::new(Browser::new(&session, context))),
    );
}

struct Browser {
    web: WebView,
    context: ComponentContext,
    pending: Rc<RefCell<Option<String>>>,
}

impl Browser {
    fn new(session: &NetworkSession, context: ComponentContext) -> Self {
        let settings = webkit6::Settings::new();
        settings.set_enable_webrtc(false);
        settings.set_enable_dns_prefetching(false);
        let web = WebView::builder()
            .network_session(session)
            .settings(&settings)
            .build();
        web.connect_decide_policy({
            let context = context.clone();
            move |_, decision, kind| {
                if matches!(
                    kind,
                    PolicyDecisionType::NavigationAction | PolicyDecisionType::NewWindowAction
                ) {
                    let allowed = decision
                        .downcast_ref::<NavigationPolicyDecision>()
                        .and_then(|navigation| navigation.navigation_action())
                        .and_then(|action| action.request())
                        .and_then(|request| request.uri())
                        .is_some_and(|uri| valid_url(&uri));
                    if !allowed {
                        decision.ignore();
                        context.emit(
                            "error",
                            JsonValue::from("Only HTTP and HTTPS pages are supported"),
                        );
                        return true;
                    }
                }
                false
            }
        });
        let pending: Rc<RefCell<Option<String>>> = Rc::default();
        web.connect_map({
            let pending = pending.clone();
            move |web| {
                if let Some(src) = pending.borrow_mut().take() {
                    web.load_uri(&src);
                }
            }
        });
        let failed = Rc::new(Cell::new(false));
        web.connect_load_changed({
            let context = context.clone();
            let failed = failed.clone();
            move |web, event| {
                if event == LoadEvent::Started {
                    failed.set(false);
                } else if event == LoadEvent::Finished && !failed.get() {
                    context.emit(
                        "loaded",
                        JsonValue::from(web.uri().map(|uri| uri.to_string())),
                    );
                }
            }
        });
        web.connect_load_failed({
            let context = context.clone();
            move |_, _, _, error| {
                failed.set(true);
                context.emit("error", JsonValue::from(error.message()));
                false
            }
        });
        web.connect_title_notify({
            let context = context.clone();
            move |web| {
                context.emit(
                    "titleChanged",
                    JsonValue::from(web.title().map(|title| title.to_string())),
                )
            }
        });
        Self {
            web,
            context,
            pending,
        }
    }
}

fn valid_url(value: &str) -> bool {
    gtk::glib::Uri::parse(value, gtk::glib::UriFlags::NONE).is_ok_and(|uri| {
        matches!(uri.scheme().as_str(), "http" | "https")
            && uri.host().is_some_and(|host| !host.is_empty())
    })
}

impl NativeComponent for Browser {
    fn widget(&self) -> gtk::Widget {
        self.web.clone().upcast()
    }

    fn set_prop(&mut self, name: &str, value: Option<&JsonValue>) {
        if name == "src" {
            match value.and_then(JsonValue::as_str) {
                Some(src) if valid_url(src) => {
                    if self.web.is_mapped() {
                        self.web.load_uri(src);
                    } else {
                        *self.pending.borrow_mut() = Some(src.to_owned());
                    }
                }
                _ => {
                    self.pending.borrow_mut().take();
                    self.web.stop_loading();
                    self.context
                        .emit("error", JsonValue::from("Invalid browser URL"));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::valid_url;

    #[test]
    fn only_http_and_https_origins_are_accepted() {
        for url in [
            "https://example.com/path?query=1",
            "http://127.0.0.1:8081/",
            "http://[::1]/",
        ] {
            assert!(valid_url(url), "{url}");
        }
        for url in [
            "",
            "example.com",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://",
        ] {
            assert!(!valid_url(url), "{url}");
        }
    }
}
