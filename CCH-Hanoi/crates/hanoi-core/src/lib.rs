// hanoi-core: Hanoi-specific CCH implementation and API surface.

pub mod geo;
pub mod graph;
pub mod guidance;
pub mod restrictions;
pub mod routing;

pub use geo::{BoundingBox, CoordRejection, SnapResult, SpatialIndex, ValidationConfig};
pub use graph::GraphData;
pub use guidance::{TurnAnnotation, TurnDirection};
pub use routing::line_graph::{LineGraphCchContext, LineGraphQueryEngine};
pub use routing::normal::{CchContext, QueryEngine};
pub use routing::QueryAnswer;