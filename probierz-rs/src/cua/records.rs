use crate::cua::*;
pub(crate) const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);
pub(crate) const LAUNCH_WAIT: Duration = Duration::from_secs(8);
pub(crate) const POLL: Duration = Duration::from_millis(400);
pub(crate) const BUNDLED_DRIVER: &str = "/Applications/CuaDriver.app/Contents/MacOS/cua-driver";

#[derive(Clone, Debug)]
pub struct Driver {
    pub(crate) binary: String,
    pub(crate) socket: PathBuf,
}

#[derive(Clone, Debug)]
pub struct App {
    pub pid: u32,
    pub window_id: u64,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub tree: String,
    pub snapshot_id: Option<String>,
    pub elements: Vec<Value>,
}

#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

