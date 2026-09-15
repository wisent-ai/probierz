//! A figure pair: what has to be present, the render itself, the geometry
//! read out of it, and the rubric score.

mod evaluate;
mod geometry;
mod render;

// Only the command is exported; the evaluation's own stages stay
// inside the package, so a name like `Router` or `Graded` cannot
// collide with another evaluation's.
pub use evaluate::evaluate_figure;
pub(crate) use geometry::*;
pub(crate) use render::*;
