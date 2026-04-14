mod context;
mod coordinate_patch;
mod engine;
mod mapping;

pub use context::LineGraphCchContext;
pub use engine::LineGraphQueryEngine;

pub(crate) use coordinate_patch::{
    clip_backtrack_protrusion_from_end, clip_backtrack_protrusion_from_start,
    update_turns_after_coordinate_patch,
};
