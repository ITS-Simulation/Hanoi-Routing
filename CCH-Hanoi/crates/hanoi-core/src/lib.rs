// hanoi-core: Hanoi-specific CCH implementation and API surface.

pub mod bounds;
pub mod cch;
mod cch_cache;
pub mod geometry;
pub mod graph;
pub mod line_graph;
pub mod multi_route;
pub mod spatial;
pub mod via_way_restriction;

// Re-export key types for ergonomic imports
pub use bounds::{BoundingBox, CoordRejection, ValidationConfig};
pub use cch::{CchContext, QueryAnswer, QueryEngine};
pub use geometry::{TurnAnnotation, TurnDirection};
pub use graph::GraphData;
pub use line_graph::{LineGraphCchContext, LineGraphQueryEngine};
pub use spatial::{SnapResult, SpatialIndex};
