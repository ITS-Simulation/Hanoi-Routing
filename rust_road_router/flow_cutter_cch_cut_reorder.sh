#!/usr/bin/env sh

seed=5489
SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
CONSOLE_BIN="${SCRIPT_DIR}/lib/InertialFlowCutter/build/console"
INPUT_DIR="$1"
GRAPH_DIR="$INPUT_DIR"

if [ ! -f "${GRAPH_DIR}/first_out" ] && [ -f "${INPUT_DIR}/graph/first_out" ]; then
  GRAPH_DIR="${INPUT_DIR}/graph"
fi

mkdir -p "${GRAPH_DIR}/perms"

"${CONSOLE_BIN}" \
   load_routingkit_unweighted_graph "${GRAPH_DIR}/first_out" "${GRAPH_DIR}/head" \
   load_routingkit_longitude "${GRAPH_DIR}/longitude" \
   load_routingkit_latitude "${GRAPH_DIR}/latitude" \
   flow_cutter_set random_seed $seed \
   reorder_nodes_at_random \
   reorder_nodes_in_preorder \
   flow_cutter_set thread_count ${2:-$(nproc)} \
   flow_cutter_set BulkDistance no \
   flow_cutter_set max_cut_size 100000000 \
   flow_cutter_set distance_ordering_cutter_count 0 \
   flow_cutter_set geo_pos_ordering_cutter_count 8 \
   flow_cutter_set bulk_assimilation_threshold 0.4 \
   flow_cutter_set bulk_assimilation_order_threshold 0.25 \
   flow_cutter_set bulk_step_fraction 0.05 \
   flow_cutter_set initial_assimilated_fraction 0.05 \
   flow_cutter_config \
   report_time \
   reorder_arcs_in_accelerated_flow_cutter_cch_order reorder\
   do_not_report_time \
   save_routingkit_arc_permutation_since_last_load "${GRAPH_DIR}/perms/cch_perm_cuts_reorder"
