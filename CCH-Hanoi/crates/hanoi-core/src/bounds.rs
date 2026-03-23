use std::fmt;

/// Axis-aligned geographic bounding box computed from graph node coordinates.
#[derive(Debug, Clone, Copy)]
pub struct BoundingBox {
    pub min_lat: f32,
    pub max_lat: f32,
    pub min_lng: f32,
    pub max_lng: f32,
}

impl BoundingBox {
    /// Compute from lat/lng slices. Panics on empty slices.
    pub fn from_coords(lat: &[f32], lng: &[f32]) -> Self {
        assert!(
            !lat.is_empty(),
            "cannot compute bounding box from empty coordinates"
        );
        let (mut min_lat, mut max_lat) = (f32::MAX, f32::MIN);
        let (mut min_lng, mut max_lng) = (f32::MAX, f32::MIN);
        for (&la, &lo) in lat.iter().zip(lng.iter()) {
            min_lat = min_lat.min(la);
            max_lat = max_lat.max(la);
            min_lng = min_lng.min(lo);
            max_lng = max_lng.max(lo);
        }
        BoundingBox {
            min_lat,
            max_lat,
            min_lng,
            max_lng,
        }
    }

    /// Check if a point is inside the box expanded by `padding_m` on all sides.
    pub fn contains_with_padding(&self, lat: f32, lng: f32, padding_m: f64) -> bool {
        let lat_pad = (padding_m / 111_320.0) as f32;
        let center_lat = ((self.min_lat + self.max_lat) / 2.0) as f64;
        let lng_pad = (padding_m / (111_320.0 * center_lat.to_radians().cos())) as f32;

        lat >= self.min_lat - lat_pad
            && lat <= self.max_lat + lat_pad
            && lng >= self.min_lng - lng_pad
            && lng <= self.max_lng + lng_pad
    }
}

/// Configurable parameters for coordinate validation.
#[derive(Debug, Clone)]
pub struct ValidationConfig {
    /// Padding in meters to expand the bounding box on all sides.
    /// Default: 1000.0 (1 km).
    pub bbox_padding_m: f64,
    /// Maximum snap distance in meters. Snaps farther than this are rejected.
    /// Default: 1000.0 (1 km).
    pub max_snap_distance_m: f64,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        ValidationConfig {
            bbox_padding_m: 1500.0,
            max_snap_distance_m: 2000.0,
        }
    }
}

/// Reason a coordinate was rejected.
#[derive(Debug, Clone)]
pub enum CoordRejection {
    /// Coordinate contains NaN or Infinity.
    NonFinite {
        label: &'static str,
        lat: f32,
        lng: f32,
    },
    /// Latitude outside [-90, 90] or longitude outside [-180, 180].
    InvalidRange {
        label: &'static str,
        lat: f32,
        lng: f32,
    },
    /// Coordinate is outside the graph's padded bounding box.
    OutOfBounds {
        label: &'static str,
        lat: f32,
        lng: f32,
        bbox: BoundingBox,
        padding_m: f64,
    },
    /// Snap distance exceeds the configured maximum.
    SnapTooFar {
        label: &'static str,
        lat: f32,
        lng: f32,
        snap_distance_m: f64,
        max_distance_m: f64,
    },
}

impl fmt::Display for CoordRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoordRejection::NonFinite { label, lat, lng } => {
                write!(f, "{label} coordinate ({lat}, {lng}) is not finite")
            }
            CoordRejection::InvalidRange { label, lat, lng } => {
                write!(
                    f,
                    "{label} coordinate ({lat}, {lng}) is outside valid geographic range"
                )
            }
            CoordRejection::OutOfBounds {
                label,
                lat,
                lng,
                bbox,
                padding_m,
            } => write!(
                f,
                "{label} coordinate ({lat}, {lng}) is outside the map's coverage area \
                    (lat [{}, {}], lng [{}, {}], {padding_m}m padding)",
                bbox.min_lat, bbox.max_lat, bbox.min_lng, bbox.max_lng
            ),
            CoordRejection::SnapTooFar {
                label,
                lat,
                lng,
                snap_distance_m,
                max_distance_m,
            } => write!(
                f,
                "{label} coordinate ({lat}, {lng}) is {snap_distance_m:.0}m from the \
                    nearest road (maximum: {max_distance_m:.0}m)"
            ),
        }
    }
}

impl CoordRejection {
    pub fn to_details_json(&self) -> serde_json::Value {
        match self {
            CoordRejection::NonFinite { label, lat, lng } => serde_json::json!({
                "reason": "non_finite", "label": label, "lat": lat, "lng": lng,
            }),
            CoordRejection::InvalidRange { label, lat, lng } => serde_json::json!({
                "reason": "invalid_range", "label": label, "lat": lat, "lng": lng,
            }),
            CoordRejection::OutOfBounds {
                label,
                lat,
                lng,
                bbox,
                padding_m,
            } => serde_json::json!({
                "reason": "out_of_bounds", "label": label, "lat": lat, "lng": lng,
                "bbox": { "min_lat": bbox.min_lat, "max_lat": bbox.max_lat,
                           "min_lng": bbox.min_lng, "max_lng": bbox.max_lng },
                "padding_m": padding_m,
            }),
            CoordRejection::SnapTooFar {
                label,
                lat,
                lng,
                snap_distance_m,
                max_distance_m,
            } => serde_json::json!({
                "reason": "snap_too_far", "label": label, "lat": lat, "lng": lng,
                "snap_distance_m": snap_distance_m, "max_distance_m": max_distance_m,
            }),
        }
    }
}

/// Validate a single coordinate against geographic range and bounding box.
pub fn validate_coordinate(
    label: &'static str,
    lat: f32,
    lng: f32,
    bbox: &BoundingBox,
    config: &ValidationConfig,
) -> Result<(), CoordRejection> {
    if !lat.is_finite() || !lng.is_finite() {
        return Err(CoordRejection::NonFinite { label, lat, lng });
    }
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lng) {
        return Err(CoordRejection::InvalidRange { label, lat, lng });
    }
    if !bbox.contains_with_padding(lat, lng, config.bbox_padding_m) {
        return Err(CoordRejection::OutOfBounds {
            label,
            lat,
            lng,
            bbox: *bbox,
            padding_m: config.bbox_padding_m,
        });
    }
    Ok(())
}
