//! The socket's directory and the daemon's exit as kqueue events. Every
//! number here is the kernel's, quoted from the header that declares it.

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

#[repr(C)]
struct KEvent {
    ident: usize,
    filter: i16,
    flags: u16,
    fflags: u32,
    data: isize,
    udata: *mut std::ffi::c_void,
}
extern "C" {
    fn kqueue() -> i32;
    fn kevent(
        kq: i32,
        changelist: *const KEvent,
        nchanges: i32,
        eventlist: *mut KEvent,
        nevents: i32,
        wait_spec: *const std::ffi::c_void,
    ) -> i32;
    fn open(path: *const std::ffi::c_char, oflag: i32, ...) -> i32;
    fn close(fd: i32) -> i32;
}
const EVFILT_VNODE: i16 = -4; // https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/event.h ("#define EVFILT_VNODE (-4)")
const EVFILT_PROC: i16 = -5; // https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/event.h ("#define EVFILT_PROC (-5)")
const EV_ADD: u16 = 0x1; // https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/event.h ("#define EV_ADD 0x0001")
const EV_CLEAR: u16 = 0x20; // https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/event.h ("#define EV_CLEAR 0x0020")
const NOTE_WRITE: u32 = 0x2; // https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/event.h ("#define NOTE_WRITE 0x00000002")
const NOTE_EXIT: u32 = 0x8000_0000; // https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/event.h ("#define NOTE_EXIT 0x80000000")
/// Open for event notification only: the descriptor reads nothing.
const O_EVTONLY: i32 = 0x8000; // https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/fcntl.h ("#define O_EVTONLY 0x00008000")
/// `kevent` answers this for a pid that has already exited.
const ESRCH: i32 = 3; // https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/errno.h ("#define ESRCH 3")

/// The length `kevent` is told for a list of changes or event slots.
fn count(list: &[KEvent]) -> io::Result<i32> {
    i32::try_from(list.len()).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
}

/// A KEvent of all zero bytes: valid, since it holds integers and a raw
/// pointer (a null udata).
fn empty() -> KEvent {
    // SAFETY: every field of KEvent accepts all zero bytes.
    unsafe { std::mem::zeroed() }
}

pub(super) enum Event {
    DirectoryChanged,
    DaemonExited,
}

pub(super) struct Watch {
    kq: i32,
    directory: i32,
    exited_early: bool,
}

impl Watch {
    pub(super) fn new(directory: &Path, pid: u32) -> io::Result<Self> {
        let path = CString::new(directory.as_os_str().as_bytes())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        // SAFETY: a valid NUL-terminated path; a negative answer is the error.
        let directory = unsafe { open(path.as_ptr(), O_EVTONLY) };
        if directory.is_negative() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: kqueue takes no arguments; a negative answer is its error.
        let kq = unsafe { kqueue() };
        if kq.is_negative() {
            let error = io::Error::last_os_error();
            // SAFETY: directory is the descriptor opened above.
            unsafe { close(directory) };
            return Err(error);
        }
        let mut watch = Watch {
            kq,
            directory,
            exited_early: false,
        };
        watch.register(directory as usize, EVFILT_VNODE, NOTE_WRITE)?;
        if let Err(error) = watch.register(pid as usize, EVFILT_PROC, NOTE_EXIT) {
            if error.raw_os_error() != Some(ESRCH) {
                return Err(error);
            }
            watch.exited_early = true;
        }
        Ok(watch)
    }

    fn register(&self, ident: usize, filter: i16, fflags: u32) -> io::Result<()> {
        // A change carries no data; its other fields are the ones set here.
        let mut change = empty();
        change.ident = ident;
        change.filter = filter;
        change.flags = EV_ADD | EV_CLEAR;
        change.fflags = fflags;
        let changes = [change];
        let slots: &mut [KEvent] = &mut [];
        // SAFETY: one valid change and no event slots, so the call returns at once.
        let answered = unsafe {
            kevent(
                self.kq,
                changes.as_ptr(),
                count(&changes)?,
                slots.as_mut_ptr(),
                count(slots)?,
                std::ptr::null(),
            )
        };
        if answered.is_negative() {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Block until the directory changes or the daemon exits.
    pub(super) fn next(&self) -> io::Result<Event> {
        if self.exited_early {
            return Ok(Event::DaemonExited);
        }
        // The kernel overwrites the one slot with the event that fired.
        let mut slots = [empty()];
        let changes: &[KEvent] = &[];
        // SAFETY: no changes and one writable event slot; a null wait spec
        // blocks until one registered event fires.
        let answered = unsafe {
            kevent(
                self.kq,
                changes.as_ptr(),
                count(changes)?,
                slots.as_mut_ptr(),
                count(&slots)?,
                std::ptr::null(),
            )
        };
        if answered.is_negative() {
            return Err(io::Error::last_os_error());
        }
        let [fired] = slots;
        Ok(if fired.filter == EVFILT_PROC {
            Event::DaemonExited
        } else {
            Event::DirectoryChanged
        })
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        // SAFETY: both are descriptors this watch opened and still owns.
        unsafe {
            close(self.kq);
            close(self.directory);
        }
    }
}
