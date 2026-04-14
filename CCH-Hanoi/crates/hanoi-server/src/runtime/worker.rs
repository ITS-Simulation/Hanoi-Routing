use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::{mpsc, watch};

use rust_road_router::datastr::graph::Weight;

use hanoi_core::{CchContext, LineGraphCchContext, LineGraphQueryEngine, QueryEngine};

use crate::api::state::QueryMsg;
use crate::runtime::dispatch::{dispatch_line_graph, dispatch_normal};

/// Background loop for the normal-graph engine.
///
/// Owns the `CchContext` and `QueryEngine` — processes queries from the mpsc
/// channel and applies customization updates from the watch channel.
pub fn run_normal(
    context: &CchContext,
    query_rx: &mut mpsc::Receiver<QueryMsg>,
    watch_rx: &mut watch::Receiver<Option<Vec<Weight>>>,
    customization_active: &Arc<AtomicBool>,
    engine_alive: &Arc<AtomicBool>,
    rt: &tokio::runtime::Handle,
) {
    let mut engine = QueryEngine::new(context);

    loop {
        // Non-blocking check for a pending customization
        if watch_rx.has_changed().unwrap_or(false) {
            if let Some(weights) = watch_rx.borrow_and_update().clone() {
                customization_active.store(true, Ordering::Relaxed);
                let _span =
                    tracing::info_span!("customization", num_weights = weights.len()).entered();
                tracing::info!(num_weights = weights.len(), "re-customizing");
                engine.update_weights(&weights);
                customization_active.store(false, Ordering::Relaxed);
                tracing::info!("customization complete");
            }
        }

        // Process one query (blocking with timeout to periodically check customization)
        let msg = rt.block_on(async {
            tokio::time::timeout(std::time::Duration::from_millis(50), query_rx.recv()).await
        });

        match msg {
            Ok(Some(qm)) => {
                let resp = dispatch_normal(
                    &mut engine,
                    qm.request,
                    qm.format.as_deref(),
                    qm.colors,
                    qm.alternatives,
                    qm.stretch,
                );
                let _ = qm.reply.send(resp);
            }
            Ok(None) => break, // Channel closed — shutdown
            Err(_) => {}       // Timeout — loop back
        }
    }

    engine_alive.store(false, Ordering::Relaxed);
}

/// Background loop for the line-graph engine.
pub fn run_line_graph(
    context: &LineGraphCchContext,
    query_rx: &mut mpsc::Receiver<QueryMsg>,
    watch_rx: &mut watch::Receiver<Option<Vec<Weight>>>,
    customization_active: &Arc<AtomicBool>,
    engine_alive: &Arc<AtomicBool>,
    rt: &tokio::runtime::Handle,
) {
    let mut engine = LineGraphQueryEngine::new(context);

    loop {
        if watch_rx.has_changed().unwrap_or(false) {
            if let Some(weights) = watch_rx.borrow_and_update().clone() {
                customization_active.store(true, Ordering::Relaxed);
                let _span =
                    tracing::info_span!("customization", num_weights = weights.len()).entered();
                tracing::info!(num_weights = weights.len(), "re-customizing line graph");
                engine.update_weights(&weights);
                customization_active.store(false, Ordering::Relaxed);
                tracing::info!("line graph customization complete");
            }
        }

        let msg = rt.block_on(async {
            tokio::time::timeout(std::time::Duration::from_millis(50), query_rx.recv()).await
        });

        match msg {
            Ok(Some(qm)) => {
                let resp = dispatch_line_graph(
                    &mut engine,
                    qm.request,
                    qm.format.as_deref(),
                    qm.colors,
                    qm.alternatives,
                    qm.stretch,
                );
                let _ = qm.reply.send(resp);
            }
            Ok(None) => break,
            Err(_) => {}
        }
    }

    engine_alive.store(false, Ordering::Relaxed);
}
