//! The budgets the screen-semantics journeys judge a pane by. These
//! constants are the contract: how many boxed frames two overlays may leave
//! in the transcript, how many rows make a two-pane view, and how long an
//! application gets to start and to paint a view.

/// Boxed frames allowed in the scrollback after opening two overlays.
pub(super) const FRAME_BUDGET_AFTER_TWO_OVERLAYS: usize = 4;
/// Rows each two-pane property must hold on.
pub(super) const MIN_TWO_PANE_ROWS: usize = 3;
/// Seconds an application gets to start, and a view to paint.
pub(super) const READY_SECONDS: u64 = 30;
pub(super) const VIEW_SECONDS: u64 = 60;
