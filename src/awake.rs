//! Keep the display awake while the game is running. macOS only.
//!
//! This is in the game rather than in the test harness because it is the game
//! that owns the window. A screenshot is a photograph of a window, and a window
//! on a sleeping display is never presented to the compositor — the capture
//! comes back as one flat colour, which is indistinguishable from a rendering
//! bug and cost an afternoon to tell apart once already. An unattended QA run
//! is exactly the situation in which nothing is touching the keyboard for
//! minutes at a time, which is exactly when the display sleeps.
//!
//! It is also what any game does while it is on screen: not idle just because
//! the input is a gamepad, or a simulation nobody is currently steering.
//!
//! # How
//!
//! An IOKit power assertion, held for the lifetime of the process:
//!
//! * `PreventUserIdleDisplaySleep` stops the *idle* timer for the display (and
//!   with it system idle sleep). It does not — deliberately cannot — stop the
//!   lid being closed, `pmset sleepnow`, or a screen lock that is already set
//!   to require a password.
//! * `IOPMAssertionDeclareUserActivity` is a one-shot nudge that wakes a
//!   display which has *already* gone to sleep, since the assertion alone only
//!   prevents the next sleep rather than undoing the last one.
//!
//! Called through `extern "C"` rather than through a crate: it is two stable C
//! functions from a framework that is already linked into every macOS binary,
//! and a dependency for that is more surface than the FFI it would hide.
//!
//! Set `CROWD2X_ALLOW_SLEEP=1` to leave the machine's power settings alone.
//!
//! Check it is working with:
//!
//! ```sh
//! pmset -g assertions | grep crowd2x
//! ```

use bevy::prelude::*;

const ENV_ALLOW_SLEEP: &str = "CROWD2X_ALLOW_SLEEP";

pub struct AwakePlugin;

impl Plugin for AwakePlugin {
    fn build(&self, app: &mut App) {
        if std::env::var(ENV_ALLOW_SLEEP).is_ok() {
            info!("awake: {ENV_ALLOW_SLEEP} set, leaving display sleep alone");
            return;
        }
        // The assertion lives in a resource so it is released when the app is
        // dropped. macOS also drops a process's assertions when it exits, so a
        // crash cannot leave the machine permanently awake either.
        #[cfg(target_os = "macos")]
        app.insert_resource(macos::StayAwake::hold());
        #[cfg(not(target_os = "macos"))]
        let _ = app;
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;
    use std::ptr;

    use bevy::prelude::*;

    type CFStringRef = *const c_void;
    type IOPMAssertionID = u32;
    type IOReturn = i32;

    /// `kIOReturnSuccess`
    const SUCCESS: IOReturn = 0;
    /// `kIOPMAssertionLevelOn`
    const LEVEL_ON: u32 = 255;
    /// `kCFStringEncodingUTF8`
    const UTF8: u32 = 0x0800_0100;
    /// `kIOPMUserActiveLocal` — activity at this machine, not a remote session.
    const USER_ACTIVE_LOCAL: i32 = 0;
    /// `kIOPMAssertionTypePreventUserIdleDisplaySleep`
    const PREVENT_DISPLAY_SLEEP: &str = "PreventUserIdleDisplaySleep";
    /// What shows up in `pmset -g assertions`, so the holder is identifiable.
    const ASSERTION_NAME: &str = "crowd2x is running";

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithBytes(
            alloc: *const c_void,
            bytes: *const u8,
            num_bytes: isize,
            encoding: u32,
            is_external_representation: u8,
        ) -> CFStringRef;
        fn CFRelease(cf: *const c_void);
    }

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOPMAssertionCreateWithName(
            assertion_type: CFStringRef,
            assertion_level: u32,
            assertion_name: CFStringRef,
            assertion_id: *mut IOPMAssertionID,
        ) -> IOReturn;
        fn IOPMAssertionRelease(assertion_id: IOPMAssertionID) -> IOReturn;
        fn IOPMAssertionDeclareUserActivity(
            assertion_name: CFStringRef,
            user_type: i32,
            assertion_id: *mut IOPMAssertionID,
        ) -> IOReturn;
    }

    /// A `CFString` that releases itself, so an early return cannot leak one.
    struct CfString(CFStringRef);

    impl CfString {
        fn new(text: &str) -> Option<Self> {
            // SAFETY: `text` is a valid UTF-8 slice and CoreFoundation copies
            // it; a null allocator means the default one.
            let raw = unsafe {
                CFStringCreateWithBytes(
                    ptr::null(),
                    text.as_ptr(),
                    text.len() as isize,
                    UTF8,
                    0,
                )
            };
            (!raw.is_null()).then_some(Self(raw))
        }
    }

    impl Drop for CfString {
        fn drop(&mut self) {
            // SAFETY: created by CFStringCreateWithBytes above and not
            // released anywhere else.
            unsafe { CFRelease(self.0) };
        }
    }

    /// The held assertion. Releasing it is what lets the display sleep again.
    #[derive(Resource)]
    pub struct StayAwake {
        assertion: Option<IOPMAssertionID>,
    }

    impl StayAwake {
        pub fn hold() -> Self {
            wake_the_display();
            Self {
                assertion: prevent_display_sleep(),
            }
        }
    }

    impl Drop for StayAwake {
        fn drop(&mut self) {
            let Some(assertion) = self.assertion.take() else {
                return;
            };
            // SAFETY: an id this process created and has not released.
            unsafe { IOPMAssertionRelease(assertion) };
        }
    }

    /// Hold the display awake until the returned id is released.
    ///
    /// A failure here is worth a line in the log and nothing more: the game
    /// runs perfectly well on a machine that dims, it just cannot be
    /// photographed unattended.
    fn prevent_display_sleep() -> Option<IOPMAssertionID> {
        let kind = CfString::new(PREVENT_DISPLAY_SLEEP)?;
        let name = CfString::new(ASSERTION_NAME)?;
        let mut assertion: IOPMAssertionID = 0;

        // SAFETY: both strings outlive the call, and `assertion` is a valid
        // out-pointer that is only read when the call reports success.
        let result = unsafe {
            IOPMAssertionCreateWithName(kind.0, LEVEL_ON, name.0, &mut assertion)
        };

        if result == SUCCESS {
            info!("awake: holding the display awake while the game runs");
            Some(assertion)
        } else {
            warn!("awake: could not stop the display sleeping (IOKit said {result})");
            None
        }
    }

    /// Wake a display that is already asleep.
    ///
    /// The assertion above only prevents the *next* sleep, so without this a
    /// run started after the machine dimmed would hold an already-dark screen
    /// awake and still photograph nothing.
    fn wake_the_display() {
        let Some(name) = CfString::new(ASSERTION_NAME) else {
            return;
        };
        let mut assertion: IOPMAssertionID = 0;

        // SAFETY: as above. The id this returns is a short assertion IOKit
        // times out on its own, so it is deliberately not kept.
        let result =
            unsafe { IOPMAssertionDeclareUserActivity(name.0, USER_ACTIVE_LOCAL, &mut assertion) };
        if result != SUCCESS {
            warn!("awake: could not wake the display (IOKit said {result})");
        }
    }
}
