use hanoi_core::{CoordRejection, LineGraphQueryEngine, QueryAnswer, QueryEngine};
use serde_json::Value;
use hanoi_core::routing::{MultiQueryRepository, QueryRepository};
use crate::api::dto::QueryRequest;
use crate::runtime::response::{format_multi_response, format_response};

fn multi_query_answer<E: MultiQueryRepository>(
    engine: &mut E,
    req: QueryRequest,
    alternatives: u32,
    stretch: f64,
) -> Result<Vec<QueryAnswer>, CoordRejection> {
    if let (Some(flat), Some(flng), Some(tlat), Some(tlng)) =
        (req.from_lat, req.from_lng, req.to_lat, req.to_lng)
    {
        engine.run_multi_query_coords((flat, flng), (tlat, tlng), alternatives as usize, stretch)
    } else if let (Some(from), Some(to)) = (req.from_node, req.to_node) {
        Ok(engine.run_multi_query(from, to, alternatives as usize, stretch))
    } else {
        Ok(Vec::new())
    }
}

fn query_answer<E: QueryRepository>(
    engine: &mut E,
    req: QueryRequest,
) -> Result<Option<QueryAnswer>, CoordRejection> {
    if let (Some(flat), Some(flng), Some(tlat), Some(tlng)) =
        (req.from_lat, req.from_lng, req.to_lat, req.to_lng)
    {
        engine.run_query_coords((flat, flng), (tlat, tlng))
    } else if let (Some(from), Some(to)) = (req.from_node, req.to_node) {
        Ok(engine.run_query(from, to))
    } else {
        Ok(None)
    }
}


pub(crate) fn dispatch_normal(
    engine: &mut QueryEngine<'_>,
    req: QueryRequest,
    format: Option<&str>,
    colors: bool,
    alternatives: u32,
    stretch: f64,
) -> Result<Value, CoordRejection> {
    if alternatives > 0 {
        let answers = multi_query_answer(engine, req, alternatives, stretch)?;

        tracing::info!(num_routes = answers.len(), "multi-route query completed");
        return Ok(format_multi_response(answers, format, colors, "normal"));
    }

    let answer = query_answer(engine, req)?;

    // Structured event with query result metadata
    match &answer {
        Some(a) => tracing::info!(
            distance_ms = a.distance_ms,
            distance_m = a.distance_m,
            path_len = a.path.len(),
            "query completed"
        ),
        None => tracing::info!("query returned no path"),
    }

    Ok(format_response(answer, format, colors, "normal"))
}

pub(crate) fn dispatch_line_graph(
    engine: &mut LineGraphQueryEngine<'_>,
    req: QueryRequest,
    format: Option<&str>,
    colors: bool,
    alternatives: u32,
    stretch: f64,
) -> Result<Value, CoordRejection> {
    if alternatives > 0 {
        let answers = multi_query_answer(engine, req, alternatives, stretch)?;

        tracing::info!(num_routes = answers.len(), "multi-route query completed");
        return Ok(format_multi_response(answers, format, colors, "line_graph"));
    }

    let answer = query_answer(engine, req)?;

    // Structured event with query result metadata
    match &answer {
        Some(a) => tracing::info!(
            distance_ms = a.distance_ms,
            distance_m = a.distance_m,
            path_len = a.path.len(),
            "query completed"
        ),
        None => tracing::info!("query returned no path"),
    }

    Ok(format_response(answer, format, colors, "line_graph"))
}
