mod index;
mod metric;
mod snap;

pub use index::SpatialIndex;
pub use metric::haversine_m;
pub use snap::SnapResult;

pub(crate) use index::SNAP_MAX_CANDIDATES;
