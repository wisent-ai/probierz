//! The driver itself, in five parts: reaching the service and starting an
//! application, acting inside one window, the windows a process owns,
//! waiting for a launched daemon's socket on kernel events, and waiting for
//! a launched app's first window on its own notification or its exit.

mod interact;
mod session;
mod socket;
mod window_watch;
mod windows;

pub(crate) use window_watch::accessibility_trusted;
