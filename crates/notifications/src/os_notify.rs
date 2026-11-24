// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Native OS notification delivery (ADR 0014). macOS uses
//! `UNUserNotificationCenter`, so the notification carries the app bundle's icon
//! and a click activates Labonair. Unbundled runs (`cargo run`) skip delivery.

use std::sync::{Arc, Mutex, OnceLock};

/// What a click on an OS notification should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClickRoute {
    /// No target: only bring the app to the front.
    BringToFront,
    /// Bring to front, then navigate to the owner-defined target string.
    Navigate(String),
}

pub fn route_click(target: Option<&str>) -> ClickRoute {
    match target {
        Some(t) if !t.is_empty() => ClickRoute::Navigate(t.to_owned()),
        _ => ClickRoute::BringToFront,
    }
}

type ClickHandler = Arc<dyn Fn(String) + Send + Sync>;

fn handler() -> &'static Mutex<Option<ClickHandler>> {
    static HANDLER: OnceLock<Mutex<Option<ClickHandler>>> = OnceLock::new();
    HANDLER.get_or_init(|| Mutex::new(None))
}

/// Registers the composition-root callback that navigates to a clicked
/// notification's target. It may be called from any thread.
pub fn set_os_click_handler(f: impl Fn(String) + Send + Sync + 'static) {
    if let Ok(mut slot) = handler().lock() {
        *slot = Some(Arc::new(f));
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn dispatch_click(target: Option<&str>) {
    if let ClickRoute::Navigate(t) = route_click(target) {
        let cb = handler().lock().ok().and_then(|slot| slot.clone());
        if let Some(cb) = cb {
            cb(t);
        }
    }
}

/// Fire-and-forget native notification. `target` is an opaque owner string
/// handed back to the click handler.
pub fn notify_os_with_target(title: &str, body: &str, target: Option<&str>) {
    #[cfg(target_os = "macos")]
    mac::deliver(title, body, target);
    // ponytail: Linux/Windows native notifications not implemented.
    #[cfg(not(target_os = "macos"))]
    let _ = (title, body, target);
}

#[cfg(target_os = "macos")]
mod mac {
    use super::dispatch_click;
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, ProtocolObject};
    use objc2::{define_class, msg_send, AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::NSApplication;
    use objc2_foundation::{
        NSBundle, NSDictionary, NSError, NSObject, NSObjectProtocol, NSString, NSUUID,
    };
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
        UNNotificationResponse, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
    };
    use std::sync::atomic::{AtomicBool, Ordering};

    const TARGET_KEY: &str = "labonair.target";

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "LabonairNotificationDelegate"]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl UNUserNotificationCenterDelegate for Delegate {
            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn did_receive(
                &self,
                _center: &UNUserNotificationCenter,
                response: &UNNotificationResponse,
                completion: &block2::Block<dyn Fn()>,
            ) {
                let info = response.notification().request().content().userInfo();
                let key = NSString::from_str(TARGET_KEY);
                let target = info
                    .objectForKey(&key)
                    .and_then(|v| v.downcast::<NSString>().ok())
                    .map(|s| s.to_string());
                if let Some(mtm) = MainThreadMarker::new() {
                    #[allow(deprecated)]
                    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
                }
                dispatch_click(target.as_deref());
                completion.call(());
            }
        }
    );

    fn bundled() -> bool {
        let bundle = NSBundle::mainBundle();
        bundle.bundleIdentifier().is_some() && bundle.bundlePath().to_string().ends_with(".app")
    }

    pub fn deliver(title: &str, body: &str, target: Option<&str>) {
        static WARNED: AtomicBool = AtomicBool::new(false);
        if !bundled() {
            if !WARNED.swap(true, Ordering::Relaxed) {
                eprintln!(
                    "labonair: OS notifications need the app bundle; skipping (unbundled run)"
                );
            }
            return;
        }
        let Some(mtm) = MainThreadMarker::new() else {
            // UNUserNotificationCenter setup is done on the main thread only.
            return;
        };
        let center = UNUserNotificationCenter::currentNotificationCenter();
        install_delegate(&center, mtm);
        // Lazy authorization: the system prompts only on the first request.
        let auth = RcBlock::new(|_granted: objc2::runtime::Bool, _err: *mut NSError| {});
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
            &auth,
        );
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setBody(&NSString::from_str(body));
        if let Some(t) = target {
            let key = NSString::from_str(TARGET_KEY);
            let val: Retained<AnyObject> = NSString::from_str(t).into();
            let info = NSDictionary::from_retained_objects(&[&*key], &[val]);
            // SAFETY: the dictionary holds only NSString keys/values.
            unsafe { content.setUserInfo(&*(Retained::as_ptr(&info) as *const NSDictionary)) };
        }
        let id = NSUUID::new().UUIDString();
        let request =
            UNNotificationRequest::requestWithIdentifier_content_trigger(&id, &content, None);
        let done = RcBlock::new(|_err: *mut NSError| {});
        center.addNotificationRequest_withCompletionHandler(&request, Some(&done));
    }

    fn install_delegate(center: &UNUserNotificationCenter, _mtm: MainThreadMarker) {
        thread_local! {
            static DELEGATE: std::cell::OnceCell<Retained<Delegate>> = const { std::cell::OnceCell::new() };
        }
        DELEGATE.with(|cell| {
            cell.get_or_init(|| {
                let d: Retained<Delegate> = unsafe { msg_send![Delegate::alloc(), init] };
                center.setDelegate(Some(ProtocolObject::from_ref(&*d)));
                d
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_without_target_only_focuses() {
        assert_eq!(route_click(None), ClickRoute::BringToFront);
        assert_eq!(route_click(Some("")), ClickRoute::BringToFront);
    }

    #[test]
    fn click_with_target_navigates() {
        assert_eq!(
            route_click(Some("pane:3")),
            ClickRoute::Navigate("pane:3".into())
        );
    }
}
