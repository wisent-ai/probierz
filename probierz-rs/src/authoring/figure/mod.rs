//! A figure pair: what has to be present, the render itself, the geometry
//! read out of it, and the rubric score.

mod evaluate;
mod geometry;
mod render;

pub use evaluate::*;
pub(crate) use geometry::*;
pub(crate) use render::*;
