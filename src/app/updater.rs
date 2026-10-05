//! Sparkle updates. Release bundles embed Sparkle.framework; development
//! builds run without it, and then no updater is available.

use objc2::{
    class, msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject, Bool},
};

pub struct Updater(Retained<AnyObject>);

impl Updater {
    /// Load Sparkle from the app bundle and start checking for updates on
    /// its schedule.
    pub fn start() -> Option<Self> {
        let nil = std::ptr::null_mut::<AnyObject>();
        // SAFETY: plain Foundation and Sparkle calls with matching
        // signatures; every returned object is checked for nil before use.
        unsafe {
            let main: *mut AnyObject = msg_send![class!(NSBundle), mainBundle];
            let frameworks: *mut AnyObject = msg_send![main, privateFrameworksPath];
            if frameworks.is_null() {
                return None;
            }
            let name: *mut AnyObject =
                msg_send![class!(NSString), stringWithUTF8String: c"Sparkle.framework".as_ptr()];
            let path: *mut AnyObject = msg_send![frameworks, stringByAppendingPathComponent: name];
            let bundle: *mut AnyObject = msg_send![class!(NSBundle), bundleWithPath: path];
            if bundle.is_null() {
                return None;
            }
            let loaded: Bool = msg_send![bundle, load];
            if !loaded.as_bool() {
                tracing::warn!(event = "updater_unavailable", "Failed to load Sparkle");
                return None;
            }
            let class = AnyClass::get(c"SPUStandardUpdaterController")?;
            let allocated: *mut AnyObject = msg_send![class, alloc];
            let controller: *mut AnyObject = msg_send![
                allocated,
                initWithStartingUpdater: Bool::YES,
                updaterDelegate: nil,
                userDriverDelegate: nil
            ];
            let updater = Retained::from_raw(controller).map(Self);
            if updater.is_some() {
                tracing::info!(event = "updater_started", "Sparkle updater started");
            }
            updater
        }
    }

    /// Check now and show the result, as the "Check for Updates…" menu
    /// item of a regular app does.
    pub fn check(&self) {
        // SAFETY: `checkForUpdates:` takes a sender object, which may be nil.
        unsafe {
            let _: () = msg_send![&*self.0, checkForUpdates: std::ptr::null_mut::<AnyObject>()];
        }
    }
}
