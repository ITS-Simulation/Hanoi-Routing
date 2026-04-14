//! Diagnostic tool for inspecting forbidden turns and via-way restrictions
//! near a specific coordinate in the original (pre-line-graph) road network.
//!
//! Usage:
//!   diagnose_turn <graph_dir> --lat 21.01 --lng 105.77 [--radius 50]
//!
//! Finds all nodes within `radius` meters of (lat, lng), then reports:
//! - Incoming/outgoing edges at each node
//! - Forbidden turn pairs involving those edges
//! - Via-way restriction chains involving those edges

use hanoi_core::via_way_restriction::load_via_way_chains;
use rust_road_router::datastr::graph::*;
use rust_road_router::io::Load;
use std::{collections::BTreeSet, error::Error, path::PathBuf};

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "diagnose_turn",
    about = "Inspect forbidden turns and via-way restrictions near a coordinate"
)]
struct Args {
    /// Graph directory (contains first_out, head, travel_time, latitude, longitude, forbidden_turn_*)
    graph_dir: PathBuf,

    /// Latitude of the inspection point
    #[arg(long)]
    lat: f64,

    /// Longitude of the inspection point
    #[arg(long)]
    lng: f64,

    /// Search radius in meters (default: 50)
    #[arg(long, default_value_t = 50.0)]
    radius: f64,

    /// Also inspect the line graph (expects <graph_dir>/line_graph/)
    #[arg(long, default_value_t = false)]
    line_graph: bool,
}

fn haversine_m(lat1: f64, lng1: f64, lat2: f64, lng2: f64) -> f64 {
    const R: f64 = 6_371_000.0;
    let (dlat, dlng) = ((lat2 - lat1).to_radians(), (lng2 - lng1).to_radians());
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlng / 2.0).sin().powi(2);
    R * 2.0 * a.sqrt().asin()
}

fn resolve_graph_dir(input_dir: &std::path::Path) -> PathBuf {
    if input_dir.join("first_out").exists() {
        return input_dir.to_path_buf();
    }
    let nested = input_dir.join("graph");
    if nested.join("first_out").exists() {
        return nested;
    }
    input_dir.to_path_buf()
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    let graph_path = resolve_graph_dir(&args.graph_dir);

    // Load graph CSR
    let first_out: Vec<EdgeId> = Vec::load_from(graph_path.join("first_out"))?;
    let head: Vec<NodeId> = Vec::load_from(graph_path.join("head"))?;
    let travel_time: Vec<Weight> = Vec::load_from(graph_path.join("travel_time"))?;
    let lat: Vec<f32> = Vec::load_from(graph_path.join("latitude"))?;
    let lng: Vec<f32> = Vec::load_from(graph_path.join("longitude"))?;

    let num_nodes = first_out.len() - 1;
    let num_arcs = head.len();

    // Build tail array
    let mut tail: Vec<NodeId> = Vec::with_capacity(num_arcs);
    for node in 0..num_nodes {
        let deg = first_out[node + 1] - first_out[node];
        for _ in 0..deg {
            tail.push(node as NodeId);
        }
    }

    // Load forbidden turns
    let forbidden_from: Vec<EdgeId> = Vec::load_from(graph_path.join("forbidden_turn_from_arc"))?;
    let forbidden_to: Vec<EdgeId> = Vec::load_from(graph_path.join("forbidden_turn_to_arc"))?;

    // Build a set for fast lookup
    let forbidden_set: BTreeSet<(EdgeId, EdgeId)> = forbidden_from
        .iter()
        .zip(forbidden_to.iter())
        .map(|(&f, &t)| (f, t))
        .collect();

    // Load via-way chains (non-fatal if missing)
    let chains = match load_via_way_chains(&graph_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Warning: could not load via-way chains: {e}");
            Vec::new()
        }
    };

    println!("Graph: {} nodes, {} arcs", num_nodes, num_arcs);
    println!("Forbidden turns: {}", forbidden_set.len());
    println!("Via-way chains: {}", chains.len());
    println!();

    // Find all nodes within radius
    let mut nearby_nodes: Vec<(NodeId, f64)> = Vec::new();
    for node in 0..num_nodes {
        let d = haversine_m(args.lat, args.lng, lat[node] as f64, lng[node] as f64);
        if d <= args.radius {
            nearby_nodes.push((node as NodeId, d));
        }
    }
    nearby_nodes.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

    if nearby_nodes.is_empty() {
        println!(
            "No nodes found within {:.0}m of ({}, {})",
            args.radius, args.lat, args.lng
        );
        println!("Try increasing --radius");
        return Ok(());
    }

    println!(
        "Found {} nodes within {:.0}m of ({}, {}):",
        nearby_nodes.len(),
        args.radius,
        args.lat,
        args.lng
    );
    println!();

    // Collect all edge IDs incident to nearby nodes (both incoming and outgoing)
    let mut nearby_edge_ids: BTreeSet<EdgeId> = BTreeSet::new();

    for &(node, dist) in &nearby_nodes {
        let start = first_out[node as usize] as usize;
        let end = first_out[node as usize + 1] as usize;
        let out_degree = end - start;

        // Count incoming edges (linear scan — acceptable for diagnostic tool)
        let in_edges: Vec<EdgeId> = (0..num_arcs as EdgeId)
            .filter(|&e| head[e as usize] == node)
            .collect();

        println!(
            "=== Node {} ({:.6}, {:.6})  dist={:.1}m  out_degree={}  in_degree={} ===",
            node,
            lat[node as usize],
            lng[node as usize],
            dist,
            out_degree,
            in_edges.len()
        );

        // List outgoing edges
        if out_degree > 0 {
            println!("  Outgoing edges:");
            for edge_id in start..end {
                let h = head[edge_id];
                let tt = travel_time[edge_id];
                println!(
                    "    edge {} -> node {} ({:.6}, {:.6})  tt={}ms",
                    edge_id, h, lat[h as usize], lng[h as usize], tt,
                );
                nearby_edge_ids.insert(edge_id as EdgeId);
            }
        }

        // List incoming edges
        if !in_edges.is_empty() {
            println!("  Incoming edges:");
            for &edge_id in &in_edges {
                let t = tail[edge_id as usize];
                let tt = travel_time[edge_id as usize];
                println!(
                    "    edge {} <- node {} ({:.6}, {:.6})  tt={}ms",
                    edge_id, t, lat[t as usize], lng[t as usize], tt,
                );
                nearby_edge_ids.insert(edge_id);
            }
        }
        println!();
    }

    // Report forbidden turns involving nearby edges
    println!("=== Forbidden turns involving nearby edges ===");
    let mut found_forbidden = 0;
    for &(from, to) in &forbidden_set {
        let from_relevant = nearby_edge_ids.contains(&from);
        let to_relevant = nearby_edge_ids.contains(&to);
        if from_relevant || to_relevant {
            let via_node = head[from as usize]; // the node where the turn happens
            let from_tail = tail[from as usize];
            let to_head = head[to as usize];
            println!(
                "  FORBIDDEN: edge {} (node {} -> {}) => edge {} (node {} -> {})",
                from, from_tail, via_node, to, via_node, to_head,
            );
            println!(
                "    path: ({:.6},{:.6}) -> ({:.6},{:.6}) -> ({:.6},{:.6})",
                lat[from_tail as usize],
                lng[from_tail as usize],
                lat[via_node as usize],
                lng[via_node as usize],
                lat[to_head as usize],
                lng[to_head as usize],
            );
            found_forbidden += 1;
        }
    }
    if found_forbidden == 0 {
        println!("  (none)");
    }
    println!();

    // Report via-way chains involving nearby edges
    println!("=== Via-way restriction chains involving nearby edges ===");
    let mut found_chains = 0;
    for (idx, chain) in chains.iter().enumerate() {
        let involves_nearby = chain.arcs.iter().any(|arc| nearby_edge_ids.contains(arc));
        if involves_nearby {
            let kind = if chain.mandatory {
                "MANDATORY"
            } else {
                "PROHIBITIVE"
            };
            println!("  Chain #{} ({}, {} arcs):", idx, kind, chain.arcs.len());
            for (i, &arc) in chain.arcs.iter().enumerate() {
                let t = tail.get(arc as usize).copied().unwrap_or(u32::MAX);
                let h = head.get(arc as usize).copied().unwrap_or(u32::MAX);
                let marker = if nearby_edge_ids.contains(&arc) {
                    " <-- nearby"
                } else {
                    ""
                };
                if t != u32::MAX && h != u32::MAX {
                    println!(
                        "    [{}/{}] edge {} : node {} ({:.6},{:.6}) -> node {} ({:.6},{:.6}){}",
                        i,
                        chain.arcs.len(),
                        arc,
                        t,
                        lat[t as usize],
                        lng[t as usize],
                        h,
                        lat[h as usize],
                        lng[h as usize],
                        marker,
                    );
                } else {
                    println!(
                        "    [{}/{}] edge {} (out of bounds){}",
                        i,
                        chain.arcs.len(),
                        arc,
                        marker
                    );
                }
            }
            found_chains += 1;
        }
    }
    if found_chains == 0 {
        println!("  (none)");
    }
    println!();

    // Report all possible turns at nearby nodes
    println!("=== Turn analysis at nearby nodes ===");
    for &(node, _) in &nearby_nodes {
        let in_edges: Vec<EdgeId> = (0..num_arcs as EdgeId)
            .filter(|&e| head[e as usize] == node)
            .collect();
        let start = first_out[node as usize] as usize;
        let end = first_out[node as usize + 1] as usize;

        if in_edges.is_empty() || start == end {
            continue;
        }

        println!(
            "  Node {} ({:.6}, {:.6}):",
            node, lat[node as usize], lng[node as usize]
        );
        for &in_edge in &in_edges {
            let from_node = tail[in_edge as usize];
            for out_edge in start..end {
                let to_node = head[out_edge];
                let is_forbidden = forbidden_set.contains(&(in_edge, out_edge as EdgeId));
                let is_uturn = from_node == to_node;

                let bearing_in = bearing(
                    lat[from_node as usize] as f64,
                    lng[from_node as usize] as f64,
                    lat[node as usize] as f64,
                    lng[node as usize] as f64,
                );
                let bearing_out = bearing(
                    lat[node as usize] as f64,
                    lng[node as usize] as f64,
                    lat[to_node as usize] as f64,
                    lng[to_node as usize] as f64,
                );
                let turn_angle = normalize_angle(bearing_out - bearing_in);
                let turn_dir = classify_turn(turn_angle);

                let status = if is_forbidden {
                    "FORBIDDEN"
                } else if is_uturn {
                    "U-TURN (allowed)"
                } else {
                    "allowed"
                };

                println!(
                    "    edge {} (from {}) -> edge {} (to {})  [{:>10}]  angle={:>7.1}°  {}",
                    in_edge, from_node, out_edge, to_node, turn_dir, turn_angle, status,
                );
            }
        }
        println!();
    }

    // Optional: line graph inspection
    if args.line_graph {
        inspect_line_graph(&args.graph_dir, &nearby_edge_ids, &lat, &lng, &tail, &head)?;
    }

    Ok(())
}

fn bearing(lat1: f64, lng1: f64, lat2: f64, lng2: f64) -> f64 {
    let (lat1, lat2) = (lat1.to_radians(), lat2.to_radians());
    let dlng = (lng2 - lng1).to_radians();
    let y = dlng.sin() * lat2.cos();
    let x = lat1.cos() * lat2.sin() - lat1.sin() * lat2.cos() * dlng.cos();
    y.atan2(x).to_degrees()
}

fn normalize_angle(mut angle: f64) -> f64 {
    while angle > 180.0 {
        angle -= 360.0;
    }
    while angle <= -180.0 {
        angle += 360.0;
    }
    angle
}

fn classify_turn(angle: f64) -> &'static str {
    let abs = angle.abs();
    if abs < 25.0 {
        "straight"
    } else if abs > 155.0 {
        "u-turn"
    } else if angle < 0.0 {
        "right"
    } else {
        "left"
    }
}

fn inspect_line_graph(
    data_dir: &std::path::Path,
    original_edge_ids: &BTreeSet<EdgeId>,
    orig_lat: &[f32],
    orig_lng: &[f32],
    orig_tail: &[NodeId],
    orig_head: &[NodeId],
) -> Result<(), Box<dyn Error>> {
    let lg_dir = if data_dir.join("line_graph/first_out").exists() {
        data_dir.join("line_graph")
    } else if data_dir.join("graph").exists() {
        // data_dir is parent, line_graph is sibling to graph
        data_dir.join("line_graph")
    } else {
        eprintln!("Could not find line_graph directory");
        return Ok(());
    };

    if !lg_dir.join("first_out").exists() {
        eprintln!("Line graph not found at {}", lg_dir.display());
        return Ok(());
    }

    let lg_first_out: Vec<EdgeId> = Vec::load_from(lg_dir.join("first_out"))?;
    let lg_head: Vec<NodeId> = Vec::load_from(lg_dir.join("head"))?;
    let lg_weight: Vec<Weight> = Vec::load_from(lg_dir.join("travel_time"))?;

    let lg_num_nodes = lg_first_out.len() - 1;

    println!("=== Line graph inspection ===");
    println!("LG nodes: {}, LG arcs: {}", lg_num_nodes, lg_head.len());
    println!();

    // In the line graph, each node corresponds to an original edge.
    // LG node `i` = original edge `i` (for base nodes; split nodes map via split_map).
    // LG edge from node `i` to node `j` means: original edge i -> original edge j is an allowed turn.
    for &orig_eid in original_edge_ids {
        if (orig_eid as usize) >= lg_num_nodes {
            continue;
        }
        let lg_node = orig_eid as usize;
        let lg_start = lg_first_out[lg_node] as usize;
        let lg_end = lg_first_out[lg_node + 1] as usize;
        let lg_degree = lg_end - lg_start;

        let t = orig_tail[orig_eid as usize];
        let h = orig_head[orig_eid as usize];

        println!(
            "  LG node {} (orig edge: node {} -> node {}, ({:.6},{:.6})->({:.6},{:.6})) out_degree={}",
            lg_node,
            t,
            h,
            orig_lat[t as usize],
            orig_lng[t as usize],
            orig_lat[h as usize],
            orig_lng[h as usize],
            lg_degree,
        );

        for lg_eid in lg_start..lg_end {
            let lg_target = lg_head[lg_eid];
            let lg_w = lg_weight[lg_eid];
            // lg_target corresponds to original edge lg_target
            if (lg_target as usize) < orig_tail.len() {
                let next_t = orig_tail[lg_target as usize];
                let next_h = orig_head[lg_target as usize];
                println!(
                    "    -> LG node {} (orig edge {} : {} -> {})  weight={}ms",
                    lg_target, lg_target, next_t, next_h, lg_w,
                );
            } else {
                // Split node
                println!(
                    "    -> LG node {} (split node)  weight={}ms",
                    lg_target, lg_w,
                );
            }
        }

        // Also check incoming LG edges (who can transition TO this original edge)
        let incoming: Vec<usize> = (0..lg_num_nodes)
            .filter(|&src| {
                let s = lg_first_out[src] as usize;
                let e = lg_first_out[src + 1] as usize;
                lg_head[s..e].contains(&(lg_node as NodeId))
            })
            .collect();

        if !incoming.is_empty() {
            println!("    Incoming LG edges from:");
            for src in incoming {
                if src < orig_tail.len() {
                    let st = orig_tail[src];
                    let sh = orig_head[src];
                    println!(
                        "      <- LG node {} (orig edge {} : {} -> {})",
                        src, src, st, sh,
                    );
                } else {
                    println!("      <- LG node {} (split node)", src);
                }
            }
        }
        println!();
    }

    Ok(())
}
