//! The two event sources on macOS: an AXObserver for the app's
//! `AXWindowCreated` notification, and a kqueue `NOTE_EXIT` on its pid
//! delivered to the same run loop through a CFFileDescriptor.

use std::cell::Cell;
use std::ffi::{c_void, CString};

type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFRunLoopRef = *const c_void;
type CFRunLoopSourceRef = *const c_void;
type CFFileDescriptorRef = *const c_void;
type AXObserverRef = *const c_void;
type AXUIElementRef = *const c_void;

#[repr(C)]
struct CFFileDescriptorContext {
    version: isize,
    info: *mut c_void,
    retain: *const c_void,
    release: *const c_void,
    copy_description: *const c_void,
}

#[repr(C)]
struct KEvent {
    ident: usize,
    filter: i16,
    flags: u16,
    fflags: u32,
    data: isize,
    udata: *mut c_void,
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXObserverCreate(
        pid: i32,
        callback: extern "C" fn(AXObserverRef, AXUIElementRef, CFStringRef, *mut c_void),
        observer: *mut AXObserverRef,
    ) -> i32;
    fn AXObserverAddNotification(
        observer: AXObserverRef,
        element: AXUIElementRef,
        notification: CFStringRef,
        refcon: *mut c_void,
    ) -> i32;
    fn AXObserverGetRunLoopSource(observer: AXObserverRef) -> CFRunLoopSourceRef;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFRunLoopDefaultMode: CFStringRef;
    fn CFRelease(object: CFTypeRef);
    fn CFStringCreateWithCString(
        allocator: CFTypeRef,
        text: *const std::ffi::c_char,
        encoding: u32,
    ) -> CFStringRef;
    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopAddSource(run_loop: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRunLoopRemoveSource(run_loop: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRunLoopRun();
    fn CFRunLoopStop(run_loop: CFRunLoopRef);
    fn CFFileDescriptorCreate(
        allocator: CFTypeRef,
        fd: i32,
        close_on_invalidate: u8,
        callout: extern "C" fn(CFFileDescriptorRef, usize, *mut c_void),
        context: *const CFFileDescriptorContext,
    ) -> CFFileDescriptorRef;
    fn CFFileDescriptorEnableCallBacks(descriptor: CFFileDescriptorRef, callbacks: usize);
    fn CFFileDescriptorCreateRunLoopSource(
        allocator: CFTypeRef,
        descriptor: CFFileDescriptorRef,
        order: isize,
    ) -> CFRunLoopSourceRef;
    fn CFFileDescriptorInvalidate(descriptor: CFFileDescriptorRef);
}

extern "C" {
    fn kqueue() -> i32;
    fn kevent(
        kq: i32,
        changelist: *const KEvent,
        nchanges: i32,
        eventlist: *mut KEvent,
        nevents: i32,
        wait_spec: *const c_void,
    ) -> i32;
    fn close(fd: i32) -> i32;
}

const UTF8: u32 = 0x0800_0100;
const READ_CALLBACK: usize = 1;
const EVFILT_PROC: i16 = -5;
const EV_ADD: u16 = 0x1;
const NOTE_EXIT: u32 = 0x8000_0000;
/// `kevent` answers this for a pid that has already exited.
const ESRCH: i32 = 3;
/// `kAXWindowCreatedNotification`, a string constant in the AX headers.
const WINDOW_CREATED: &str = "AXWindowCreated";

pub(super) enum Event {
    WindowCreated,
    Exited,
}

/// What the run loop's callbacks saw, read after `CFRunLoopRun` returns.
#[derive(Default)]
struct Seen {
    window: Cell<bool>,
    exited: Cell<bool>,
}

pub(super) struct Watch {
    seen: Box<Seen>,
    exited_early: bool,
    run_loop: CFRunLoopRef,
    observer: AXObserverRef,
    application: AXUIElementRef,
    notification: CFStringRef,
    observer_source: CFRunLoopSourceRef,
    exit_descriptor: CFFileDescriptorRef,
    exit_source: CFRunLoopSourceRef,
}

extern "C" fn window_created(
    _: AXObserverRef,
    _: AXUIElementRef,
    _: CFStringRef,
    refcon: *mut c_void,
) {
    // SAFETY: refcon is the boxed `Seen` the watch owns while the observer
    // is registered on this thread's run loop.
    let seen = unsafe { &*(refcon as *const Seen) };
    seen.window.set(true);
    // SAFETY: called on the thread whose run loop is running.
    unsafe { CFRunLoopStop(CFRunLoopGetCurrent()) };
}

extern "C" fn process_exited(_: CFFileDescriptorRef, _: usize, info: *mut c_void) {
    // SAFETY: info is the boxed `Seen`, as above.
    let seen = unsafe { &*(info as *const Seen) };
    seen.exited.set(true);
    // SAFETY: called on the thread whose run loop is running.
    unsafe { CFRunLoopStop(CFRunLoopGetCurrent()) };
}

/// Whether this process holds the Accessibility grant `Watch` needs.
pub(super) fn trusted() -> bool {
    // SAFETY: a plain query of this process's Accessibility trust.
    unsafe { AXIsProcessTrusted() != 0 }
}

/// A kqueue holding `NOTE_EXIT` for `pid`, and whether the pid was already gone.
fn exit_queue(pid: u32) -> Result<(i32, bool), String> {
    // SAFETY: kqueue takes no arguments; a negative answer is its error.
    let kq = unsafe { kqueue() };
    if kq < 0 {
        return Err(format!(
            "cannot watch pid {pid}'s exit: {}",
            std::io::Error::last_os_error()
        ));
    }
    let change = KEvent {
        ident: pid as usize,
        filter: EVFILT_PROC,
        flags: EV_ADD,
        fflags: NOTE_EXIT,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    // SAFETY: one valid change and no event slots, so the call returns at once.
    if unsafe { kevent(kq, &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) } >= 0 {
        return Ok((kq, false));
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(ESRCH) {
        return Ok((kq, true));
    }
    // SAFETY: kq is the descriptor opened above.
    unsafe { close(kq) };
    Err(format!("cannot watch pid {pid}'s exit: {error}"))
}

impl Watch {
    pub(super) fn new(pid: u32) -> Result<Self, String> {
        // SAFETY: a plain query of this process's Accessibility trust.
        if unsafe { AXIsProcessTrusted() } == 0 {
            return Err(format!(
                "probierz is not trusted for Accessibility, so it cannot observe pid {pid} open its window; \
                 grant the program that runs probierz in System Settings > Privacy & Security > Accessibility"
            ));
        }
        let seen = Box::<Seen>::default();
        let refcon = &*seen as *const Seen as *mut c_void;
        let (kq, exited_early) = exit_queue(pid)?;
        let context = CFFileDescriptorContext {
            version: 0,
            info: refcon,
            retain: std::ptr::null(),
            release: std::ptr::null(),
            copy_description: std::ptr::null(),
        };
        let name = CString::new(WINDOW_CREATED).expect("a constant without NUL");
        // SAFETY: every call receives objects created here; each is released
        // once in Drop, which runs on every early return below.
        unsafe {
            let run_loop = CFRunLoopGetCurrent();
            let exit_descriptor =
                CFFileDescriptorCreate(std::ptr::null(), kq, 1, process_exited, &context);
            if exit_descriptor.is_null() {
                close(kq);
                return Err(format!("cannot watch pid {pid}'s exit on the run loop"));
            }
            CFFileDescriptorEnableCallBacks(exit_descriptor, READ_CALLBACK);
            let exit_source =
                CFFileDescriptorCreateRunLoopSource(std::ptr::null(), exit_descriptor, 0);
            CFRunLoopAddSource(run_loop, exit_source, kCFRunLoopDefaultMode);
            let mut watch = Watch {
                seen,
                exited_early,
                run_loop,
                observer: std::ptr::null(),
                application: AXUIElementCreateApplication(pid as i32),
                notification: CFStringCreateWithCString(std::ptr::null(), name.as_ptr(), UTF8),
                observer_source: std::ptr::null(),
                exit_descriptor,
                exit_source,
            };
            if exited_early {
                return Ok(watch);
            }
            let mut observer: AXObserverRef = std::ptr::null();
            let created = AXObserverCreate(pid as i32, window_created, &mut observer);
            if created != 0 || observer.is_null() {
                return Err(format!("cannot observe pid {pid}'s windows: AXObserverCreate answered AXError {created}"));
            }
            watch.observer = observer;
            // The AX message is answered by the app's own run loop; an app that
            // cannot answer it is named with the system's error, not a limit of ours.
            let added =
                AXObserverAddNotification(observer, watch.application, watch.notification, refcon);
            if added != 0 {
                return Err(format!(
                    "pid {pid} did not accept a window-created observer: AXError {added}"
                ));
            }
            watch.observer_source = AXObserverGetRunLoopSource(observer);
            CFRunLoopAddSource(run_loop, watch.observer_source, kCFRunLoopDefaultMode);
            Ok(watch)
        }
    }

    /// Run this thread's run loop until the app creates a window or exits.
    pub(super) fn next(&self) -> Result<Event, String> {
        if self.exited_early || self.seen.exited.get() {
            return Ok(Event::Exited);
        }
        self.seen.window.set(false);
        // SAFETY: the run loop holds the sources registered in `new`; each
        // callback stops it.
        unsafe { CFRunLoopRun() };
        if self.seen.exited.get() {
            return Ok(Event::Exited);
        }
        if self.seen.window.get() {
            return Ok(Event::WindowCreated);
        }
        Err(
            "the run loop watching for a window stopped with neither a window nor an exit"
                .to_string(),
        )
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        // SAFETY: each object was created in `new` and is released once here.
        unsafe {
            if !self.observer_source.is_null() {
                CFRunLoopRemoveSource(self.run_loop, self.observer_source, kCFRunLoopDefaultMode);
            }
            CFRunLoopRemoveSource(self.run_loop, self.exit_source, kCFRunLoopDefaultMode);
            CFRelease(self.exit_source);
            // Invalidating closes the kqueue descriptor it was created with.
            CFFileDescriptorInvalidate(self.exit_descriptor);
            CFRelease(self.exit_descriptor);
            for object in [self.observer, self.application, self.notification] {
                if !object.is_null() {
                    CFRelease(object);
                }
            }
        }
    }
}
