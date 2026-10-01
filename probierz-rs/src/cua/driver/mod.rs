//! The driver itself, in four parts: reaching the service and starting an
//! application, acting inside one window, the windows a process owns, and
//! waiting for a launched daemon's socket on kernel events.

mod interact;
mod session;
mod socket;
mod windows;
