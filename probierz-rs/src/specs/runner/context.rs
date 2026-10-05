use crate::specs::*;

pub(crate) fn at_iso(base: SystemTime, elapsed: Duration) -> String {
    iso_timestamp(base + elapsed)
}
