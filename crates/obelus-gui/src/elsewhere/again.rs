//! Obelus opened again, from Finder, while one is already open.
//!
//! Launch Services starts an application once. Opening it again -- from
//! Finder, Spotlight, `open -a` -- starts nothing: the one that is
//! running is sent the reopen event and is expected to put up a window of
//! its own. Obelus has one window to a process (several Obelus processes is
//! the normal case), so a second window is a second process, and the event
//! is answered by starting one -- the same `obg` a key in a window starts
//! for another tree, but on no tree, so that it asks which project as the
//! first one did.
//!
//! Except from the Dock, which sends the same event when its icon is
//! clicked. Each process has an icon of its own there, and a click on one
//! means *that* window: a second window on every click would be a window
//! for every time the reader came back to this one. So the event says who
//! sent it, and the Dock is answered by bringing this window forward,
//! which is what AppKit's own answer did for a window that was minimised.
//!
//! Launchpad is not on the list above, because it runs inside the Dock's
//! process and may well be taken for it: an Obelus opened from there while
//! one is open is likely to bring that one forward rather than start
//! another. Nothing in the event tells the two apart.

use objc2::{
    DefinedClass as _, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, NSObject},
    sel,
};
use objc2_foundation::NSString;
use winit::event_loop::EventLoopProxy;

use crate::window::Waking;

/// `kCoreEventClass`, `'aevt'`.
const CORE: u32 = u32::from_be_bytes(*b"aevt");
/// `kAEReopenApplication`, `'rapp'`.
const REOPEN: u32 = u32::from_be_bytes(*b"rapp");
/// `keySenderPIDAttr`, `'spid'`: which process sent the event.
const SENDER: u32 = u32::from_be_bytes(*b"spid");
/// Who sends the event for a click on the icon in the Dock.
const DOCK: &str = "com.apple.dock";

/// What the listener holds.
struct Held {
    proxy: EventLoopProxy<Waking>,
}

define_class!(
    // Safety: `NSObject` has no subclassing requirements, and nothing here
    // implements `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ObelusOpenedAgain"]
    #[ivars = Held]
    struct Listener;

    impl Listener {
        // The reply as an `Option`: nothing promises it is not nil, and a
        // reference is taken from the pointer without asking.
        #[unsafe(method(reopened:withReply:))]
        fn reopened(&self, event: &AnyObject, _reply: Option<&AnyObject>) {
            let sender = sent_by(event);
            // Said either way: which of the two it was taken for is the
            // whole of what this decides, and the sender is a helper of
            // Launch Services' more often than the program the reader used.
            tracing::info!(?sender, "opened again");
            match sender.as_deref() {
                Some(DOCK) => {
                    let _ = self.ivars().proxy.send_event(Waking::ComeForward(None));
                }
                _ => super::open(None, None),
            }
        }
    }
);

/// Answers the reopen event from here on, for as long as the process runs.
///
/// Once there is a window: AppKit puts its own answer in place as it
/// finishes launching, and the last one set is the one asked.
pub(crate) fn listen(proxy: EventLoopProxy<Waking>) {
    let Some(main) = MainThreadMarker::new() else {
        tracing::warn!("asked to listen for the reopen event off the main thread");
        return;
    };
    let listener = Listener::alloc(main).set_ivars(Held { proxy });
    // Safety: `NSObject`'s own initialiser.
    let listener: Retained<Listener> = unsafe { msg_send![super(listener), init] };
    let manager: Retained<AnyObject> =
        unsafe { msg_send![objc2::class!(NSAppleEventManager), sharedAppleEventManager] };
    // Safety: the selector is the listener's, and takes the two
    // descriptors an Apple event handler is called with.
    unsafe {
        let _: () = msg_send![
            &*manager,
            setEventHandler: &*listener,
            andSelector: sel!(reopened:withReply:),
            forEventClass: CORE,
            andEventID: REOPEN,
        ];
    }
    // The manager does not hold what it calls, and this is asked until the
    // process ends.
    std::mem::forget(listener);
}

/// The bundle of the application that sent an Apple event.
fn sent_by(event: &AnyObject) -> Option<String> {
    // Safety: an `NSAppleEventDescriptor`, which is what an Apple event
    // handler is called with, and its attribute is another.
    let sender: Option<Retained<AnyObject>> =
        unsafe { msg_send![event, attributeDescriptorForKeyword: SENDER] };
    let pid: i32 = unsafe { msg_send![&*sender?, int32Value] };
    // Safety: a class method of AppKit's, which is nil for a process that
    // is not an application.
    let running: Option<Retained<AnyObject>> = unsafe {
        msg_send![
            objc2::class!(NSRunningApplication),
            runningApplicationWithProcessIdentifier: pid
        ]
    };
    let bundle: Option<Retained<NSString>> = unsafe { msg_send![&*running?, bundleIdentifier] };
    bundle.map(|bundle| bundle.to_string())
}
