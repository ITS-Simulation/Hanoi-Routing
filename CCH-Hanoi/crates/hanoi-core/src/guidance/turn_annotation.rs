use serde::Serialize;

/// Classification of a turn maneuver at an intersection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnDirection {
    Straight,
    SlightLeft,
    SlightRight,
    Left,
    Right,
    SharpLeft,
    SharpRight,
    UTurn,
    /// Entering a roundabout.
    RoundaboutEnter,
    /// Exit roundabout going straight through.
    RoundaboutExitStraight,
    /// Exit roundabout to the right (first exit in right-hand traffic).
    RoundaboutExitRight,
    /// Exit roundabout to the left (go around most of the ring).
    RoundaboutExitLeft,
    /// Exit roundabout back the way you came (full loop).
    RoundaboutExitUturn,
}

/// A single turn annotation along a route.
#[derive(Debug, Clone, Serialize)]
pub struct TurnAnnotation {
    /// Classified turn direction.
    pub direction: TurnDirection,

    /// Signed turn angle in degrees. Positive = left, negative = right.
    /// Range: [-180, 180].
    pub angle_degrees: f64,

    /// Index into the output `coordinates[]` array where this maneuver occurs.
    #[serde(skip)]
    pub coordinate_index: u32,

    /// Distance in meters from this maneuver to the next maneuver (or to the
    /// route end for the last entry).
    pub distance_to_next_m: f64,

    /// Degree of the intersection node in the original graph (outgoing edges).
    #[serde(skip)]
    pub intersection_degree: u32,

    /// Whether this turn involves a roundabout arc (used for roundabout grouping).
    #[serde(skip)]
    pub is_roundabout: bool,
}
