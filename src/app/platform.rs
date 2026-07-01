#[cfg(target_os = "macos")]
use objc2::{class, msg_send};

pub fn activate_as_accessory() {
    #[cfg(target_os = "macos")]
    unsafe {
        let ns_app: *mut objc2::runtime::AnyObject =
            msg_send![class!(NSApplication), sharedApplication];
        let _: bool = msg_send![ns_app, setActivationPolicy: 1i64];
    }
}
