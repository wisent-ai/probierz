//! The GAC hooks: the fixtures they generate, the render a fixture is turned
//! into, and the visual evaluation of that render.

mod evaluate;
mod fixtures;
mod render;

pub(crate) use evaluate::*;
pub(crate) use fixtures::*;
pub(crate) use render::*;
