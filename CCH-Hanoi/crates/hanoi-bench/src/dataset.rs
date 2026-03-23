use std::path::Path;

use rand::Rng;
use rand::SeedableRng;
use rand::rngs::StdRng;

/// A query pair (source, destination) with optional coordinates.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct QueryPair {
    pub from_node: u32,
    pub to_node: u32,
    pub from_coords: Option<(f32, f32)>,
    pub to_coords: Option<(f32, f32)>,
}

/// Generate random query pairs from graph metadata.
///
/// Picks random node IDs in `[0, num_nodes)` and attaches coordinates
/// from the provided latitude/longitude arrays.
pub fn generate_random_queries(
    num_nodes: u32,
    lat: &[f32],
    lng: &[f32],
    count: usize,
    seed: u64,
) -> Vec<QueryPair> {
    let mut rng = StdRng::seed_from_u64(seed);
    (0..count)
        .map(|_| {
            let from_node = rng.random_range(0..num_nodes);
            let to_node = rng.random_range(0..num_nodes);
            let from_coords = if (from_node as usize) < lat.len() {
                Some((lat[from_node as usize], lng[from_node as usize]))
            } else {
                None
            };
            let to_coords = if (to_node as usize) < lat.len() {
                Some((lat[to_node as usize], lng[to_node as usize]))
            } else {
                None
            };
            QueryPair {
                from_node,
                to_node,
                from_coords,
                to_coords,
            }
        })
        .collect()
}

/// Load query pairs from a JSON file (for reproducible runs).
pub fn load_queries(path: &Path) -> Vec<QueryPair> {
    let data = std::fs::read_to_string(path).expect("failed to read query file");
    serde_json::from_str(&data).expect("failed to parse query JSON")
}

/// Save query pairs to a JSON file.
pub fn save_queries(queries: &[QueryPair], path: &Path) {
    let data = serde_json::to_string_pretty(queries).expect("failed to serialize queries");
    std::fs::write(path, data).expect("failed to write query file");
}
