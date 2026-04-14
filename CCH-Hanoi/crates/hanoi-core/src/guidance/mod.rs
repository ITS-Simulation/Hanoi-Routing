mod turn_annotation;
mod turn_classify;
mod turn_refine;

pub use turn_annotation::{TurnAnnotation, TurnDirection};
pub use turn_classify::{classify_turn, compute_turn_angle};
pub use turn_refine::{compute_turns, refine_turns};

pub(crate) use turn_classify::{
    SHARP_THRESHOLD_DEG, SLIGHT_THRESHOLD_DEG, STRAIGHT_THRESHOLD_DEG, U_TURN_THRESHOLD_DEG,
};
pub(crate) use turn_refine::annotate_distances;
