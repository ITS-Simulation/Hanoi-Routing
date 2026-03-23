#include <routingkit/conditional_restriction_resolver.h>
#include <routingkit/osm_graph_builder.h>
#include <routingkit/osm_profile.h>
#include <routingkit/id_mapper.h>
#include <routingkit/vector_io.h>
#include <routingkit/constants.h>
#include <routingkit/sort.h>
#include <routingkit/permutation.h>
#include <routingkit/inverse_vector.h>
#include <routingkit/graph_util.h>
#include <routingkit/filter.h>
#include <routingkit/bit_vector.h>

#include <algorithm>
#include <cmath>
#include <cassert>
#include <stdexcept>
#include <string>

namespace RoutingKit{

namespace{

const float pi = 3.14159265359f;

float mod_2pi(float angle){
	while(angle < 0.0f)
		angle += 2.0f*pi;
	while(angle > 2.0f*pi)
		angle -= 2.0f*pi;
	return angle;
}

bool angle_matches_direction(float angle_diff, OSMTurnDirection direction){
	switch(direction){
		case OSMTurnDirection::left_turn:
			return (pi*1.0f/4.0f < angle_diff && angle_diff < pi*3.0f/4.0f);
		case OSMTurnDirection::right_turn:
			return (pi*5.0f/4.0f < angle_diff && angle_diff < pi*7.0f/4.0f);
		case OSMTurnDirection::straight_on:
			return (angle_diff < pi/3.0f || 5.0f*pi/3.0f < angle_diff);
		case OSMTurnDirection::u_turn:
			return (2.0f*pi/3.0f < angle_diff && angle_diff < 4.0f*pi/3.0f);
	}
	return false;
}

struct GraphData{
	std::vector<unsigned> first_out;
	std::vector<unsigned> head;
	std::vector<unsigned> way;
	std::vector<float> latitude;
	std::vector<float> longitude;
	std::vector<unsigned> tail;

	// Precomputed arc-by-way index
	std::vector<unsigned> index_to_arc;
	std::vector<unsigned> first_index_of_way;

	// Precomputed incoming arc index
	std::vector<unsigned> in_arc;
	std::vector<unsigned> first_in;

	unsigned node_count() const { return first_out.empty() ? 0 : first_out.size() - 1; }
	unsigned arc_count() const { return head.size(); }
};

GraphData load_graph(const std::string& graph_dir, std::function<void(const std::string&)> log_message){
	GraphData g;

	if(log_message) log_message("Loading graph from " + graph_dir);

	g.first_out = load_vector<unsigned>(graph_dir + "/first_out");
	g.head = load_vector<unsigned>(graph_dir + "/head");
	g.way = load_vector<unsigned>(graph_dir + "/way");
	g.latitude = load_vector<float>(graph_dir + "/latitude");
	g.longitude = load_vector<float>(graph_dir + "/longitude");

	unsigned nc = g.node_count();
	unsigned ac = g.arc_count();

	if(g.way.size() != ac){
		throw std::runtime_error(
			"Graph metadata mismatch in \"" + graph_dir + "\": way.size()="
			+ std::to_string(g.way.size()) + " but arc_count=" + std::to_string(ac)
		);
	}
	if(g.latitude.size() != nc || g.longitude.size() != nc){
		throw std::runtime_error(
			"Graph coordinate mismatch in \"" + graph_dir + "\": node_count="
			+ std::to_string(nc) + ", latitude.size()=" + std::to_string(g.latitude.size())
			+ ", longitude.size()=" + std::to_string(g.longitude.size())
		);
	}

	if(log_message) log_message("Graph has " + std::to_string(nc) + " nodes and " + std::to_string(ac) + " arcs");

	// Build tail array
	g.tail.resize(ac);
	for(unsigned v = 0; v < nc; ++v)
		for(unsigned a = g.first_out[v]; a < g.first_out[v+1]; ++a)
			g.tail[a] = v;

	// Build way→arc index
	unsigned way_count = 0;
	for(unsigned a = 0; a < ac; ++a)
		if(g.way[a] >= way_count)
			way_count = g.way[a] + 1;

	g.index_to_arc = compute_sort_permutation_using_key(g.way, way_count, [](unsigned x){ return x; });
	g.first_index_of_way = invert_vector(apply_permutation(g.index_to_arc, g.way), way_count);

	// Build incoming arc index
	g.in_arc = compute_sort_permutation_using_key(g.head, nc, [](unsigned x){ return x; });
	g.first_in = invert_vector(apply_permutation(g.in_arc, g.head), nc);

	return g;
}

// Finds arcs on a given routing way that are incoming to a given node (head == node).
std::vector<unsigned> find_incoming_arcs_of_way_at_node(const GraphData& g, unsigned routing_way, unsigned node){
	std::vector<unsigned> result;
	for(unsigned i = g.first_in[node]; i < g.first_in[node+1]; ++i){
		unsigned arc = g.in_arc[i];
		if(g.way[arc] == routing_way)
			result.push_back(arc);
	}
	return result;
}

// Finds arcs on a given routing way that are outgoing from a given node (tail == node).
std::vector<unsigned> find_outgoing_arcs_of_way_at_node(const GraphData& g, unsigned routing_way, unsigned node){
	std::vector<unsigned> result;
	for(unsigned a = g.first_out[node]; a < g.first_out[node+1]; ++a){
		if(g.way[a] == routing_way)
			result.push_back(a);
	}
	return result;
}

// Finds the shared routing node between two routing ways.
// Returns invalid_id if 0 or 2+ candidates.
unsigned find_junction_node(const GraphData& g, unsigned way_a, unsigned way_b){
	if(g.first_index_of_way.size() <= 1)
		return invalid_id;

	std::size_t way_count = g.first_index_of_way.size() - 1;
	if(way_a >= way_count || way_b >= way_count)
		return invalid_id;

	unsigned nc = g.node_count();
	std::vector<bool> is_endpoint_of_a(nc, false);

	for(unsigned i = g.first_index_of_way[way_a]; i < g.first_index_of_way[way_a+1]; ++i){
		unsigned arc = g.index_to_arc[i];
		is_endpoint_of_a[g.head[arc]] = true;
		is_endpoint_of_a[g.tail[arc]] = true;
	}

	unsigned junction = invalid_id;
	unsigned junction_count = 0;
	for(unsigned i = g.first_index_of_way[way_b]; i < g.first_index_of_way[way_b+1]; ++i){
		unsigned arc = g.index_to_arc[i];
		for(unsigned node : {g.tail[arc], g.head[arc]}){
			if(is_endpoint_of_a[node]){
				if(junction != node){
					if(junction == invalid_id){
						junction = node;
						++junction_count;
					} else {
						++junction_count;
					}
				}
			}
		}
	}

	if(junction_count != 1)
		return invalid_id;
	return junction;
}

struct ArcPairResult{
	bool valid;
	unsigned from_arc;
	unsigned to_arc;
};

// Resolves a from_arc/to_arc pair at a given via_node, using angle disambiguation.
// from_candidates: arcs on from_way incoming to via_node (head == via_node)
// to_candidates: arcs on to_way outgoing from via_node (tail == via_node)
ArcPairResult disambiguate_arc_pair(
	const GraphData& g,
	const std::vector<unsigned>& from_candidates,
	const std::vector<unsigned>& to_candidates,
	unsigned via_node,
	OSMTurnDirection direction
){
	ArcPairResult result;
	result.valid = false;

	if(from_candidates.empty() || to_candidates.empty())
		return result;

	if(from_candidates.size() == 1 && to_candidates.size() == 1){
		result.valid = true;
		result.from_arc = from_candidates[0];
		result.to_arc = to_candidates[0];
		return result;
	}

	float via_lat = g.latitude[via_node];
	float via_lon = g.longitude[via_node];

	unsigned matching_count = 0;

	for(unsigned from_cand : from_candidates){
		// Use tail node position (Option A — routing node positions only)
		float from_lat = g.latitude[g.tail[from_cand]];
		float from_lon = g.longitude[g.tail[from_cand]];
		float from_angle = atan2(via_lat - from_lat, via_lon - from_lon);

		for(unsigned to_cand : to_candidates){
			float to_lat = g.latitude[g.head[to_cand]];
			float to_lon = g.longitude[g.head[to_cand]];
			float to_angle = atan2(to_lat - via_lat, to_lon - via_lon);
			float angle_diff = mod_2pi(to_angle - from_angle);

			if(angle_matches_direction(angle_diff, direction)){
				++matching_count;
				result.from_arc = from_cand;
				result.to_arc = to_cand;
			}
		}
	}

	if(matching_count == 1)
		result.valid = true;

	return result;
}

// For interior/exit via-way junctions, angle disambiguation using the overall
// restriction direction is invalid (the direction describes the full
// from_way→to_way turn, not each local chain step). Accept only when both
// candidate sets have exactly one arc.
ArcPairResult resolve_unique_arc_pair(
	const std::vector<unsigned>& from_candidates,
	const std::vector<unsigned>& to_candidates
){
	ArcPairResult result;
	result.valid = false;
	if(from_candidates.size() == 1 && to_candidates.size() == 1){
		result.valid = true;
		result.from_arc = from_candidates[0];
		result.to_arc = to_candidates[0];
	}
	return result;
}

// Adds resolved turn pair(s) for a prohibitive restriction
void add_prohibitive_turn(
	unsigned from_arc, unsigned to_arc,
	const std::string& condition,
	std::vector<unsigned>& out_from,
	std::vector<unsigned>& out_to,
	std::vector<std::string>& out_cond
){
	out_from.push_back(from_arc);
	out_to.push_back(to_arc);
	out_cond.push_back(condition);
}

// Adds resolved turn pair(s) for a mandatory restriction at a via_node:
// Forbids all outgoing arcs at via_node except to_arc, for the given from_arc.
void add_mandatory_turn(
	unsigned from_arc, unsigned to_arc, unsigned via_node,
	const GraphData& g,
	const std::string& condition,
	std::vector<unsigned>& out_from,
	std::vector<unsigned>& out_to,
	std::vector<std::string>& out_cond
){
	for(unsigned a = g.first_out[via_node]; a < g.first_out[via_node+1]; ++a){
		if(a != to_arc){
			out_from.push_back(from_arc);
			out_to.push_back(a);
			out_cond.push_back(condition);
		}
	}
}

// Walk arcs along a way from a start arc to a target node, following the way's
// arc chain through intermediate nodes. Returns the sequence of arcs traversed
// (excluding start_arc itself, which is already accounted for by the caller).
// Returns empty if the target node is not reachable via the way from start_arc,
// or if a cycle is detected.
std::vector<unsigned> walk_way_arcs(
	const GraphData& g,
	unsigned start_arc,
	unsigned target_node,
	unsigned routing_way
){
	std::vector<unsigned> arcs;
	unsigned current_node = g.head[start_arc];
	unsigned max_steps = g.arc_count(); // safety bound

	while(current_node != target_node && max_steps-- > 0){
		auto out = find_outgoing_arcs_of_way_at_node(g, routing_way, current_node);
		if(out.size() != 1)
			return {}; // Ambiguous or dead-end: cannot walk uniquely
		arcs.push_back(out[0]);
		current_node = g.head[out[0]];
	}

	if(current_node != target_node)
		return {}; // Did not reach target

	return arcs;
}

} // anonymous namespace

std::vector<ViaWayChain> resolve_via_way_chains(
	const std::string& graph_dir,
	const std::string& pbf_file,
	const std::vector<RawConditionalRestriction>& raw_restrictions,
	std::function<bool(uint64_t, const TagMap&)>is_way_used,
	std::function<void(const std::string&)> log_message
){
	std::vector<ViaWayChain> chains;

	if(!is_way_used){
		is_way_used = [](uint64_t id, const TagMap&tags){ return is_osm_way_used_by_cars(id, tags); };
	}

	// Filter to unconditional via-way restrictions only
	std::vector<const RawConditionalRestriction*> via_way_raw;
	for(const auto& r : raw_restrictions){
		if(!r.via_ways.empty() && r.condition_string.empty())
			via_way_raw.push_back(&r);
	}

	if(via_way_raw.empty()){
		if(log_message) log_message("No unconditional via-way restrictions to resolve into chains");
		return chains;
	}

	if(log_message) log_message("Resolving " + std::to_string(via_way_raw.size()) + " unconditional via-way restrictions into arc chains...");

	auto g = load_graph(graph_dir, log_message);

	auto mapping = load_osm_id_mapping_from_pbf(
		pbf_file,
		[](uint64_t, const TagMap&){ return false; },
		[&](uint64_t osm_way_id, const TagMap& tags){
			return is_way_used(osm_way_id, tags);
		},
		log_message
	);

	IDMapper routing_node(mapping.is_routing_node);
	IDMapper routing_way(mapping.is_routing_way);

	unsigned resolved_count = 0;
	unsigned dropped_count = 0;

	for(const auto* rp : via_way_raw){
		const auto& r = *rp;

		unsigned local_from_way = routing_way.to_local(r.from_way, invalid_id);
		unsigned local_to_way = routing_way.to_local(r.to_way, invalid_id);
		if(local_from_way == invalid_id || local_to_way == invalid_id){
			++dropped_count;
			continue;
		}

		// Build way chain: [from_way, via_way_1, ..., via_way_n, to_way]
		std::vector<unsigned> way_chain;
		std::vector<uint64_t> osm_way_chain;
		way_chain.reserve(r.via_ways.size() + 2);
		osm_way_chain.reserve(r.via_ways.size() + 2);

		way_chain.push_back(local_from_way);
		osm_way_chain.push_back(r.from_way);
		bool dropped = false;
		for(uint64_t osm_via_way : r.via_ways){
			unsigned local_via_way = routing_way.to_local(osm_via_way, invalid_id);
			if(local_via_way == invalid_id){
				if(log_message)
					log_message("Via-way chain (relation " + std::to_string(r.osm_relation_id) + "): via-way " + std::to_string(osm_via_way) + " not in routing graph, dropping");
				dropped = true;
				break;
			}
			way_chain.push_back(local_via_way);
			osm_way_chain.push_back(osm_via_way);
		}
		if(dropped){ ++dropped_count; continue; }
		way_chain.push_back(local_to_way);
		osm_way_chain.push_back(r.to_way);

		// Build junction chain
		std::vector<unsigned> junctions;
		junctions.reserve(way_chain.size() - 1);
		bool junction_chain_invalid = false;
		for(std::size_t i = 0; i + 1 < way_chain.size(); ++i){
			unsigned junction = find_junction_node(g, way_chain[i], way_chain[i+1]);
			if(junction == invalid_id){
				if(log_message)
					log_message("Via-way chain (relation " + std::to_string(r.osm_relation_id) + "): no unique junction between way " + std::to_string(osm_way_chain[i]) + " and way " + std::to_string(osm_way_chain[i+1]) + ", dropping");
				junction_chain_invalid = true;
				break;
			}
			if(std::find(junctions.begin(), junctions.end(), junction) != junctions.end()){
				if(log_message)
					log_message("Via-way chain (relation " + std::to_string(r.osm_relation_id) + "): junction reuse at node " + std::to_string(routing_node.to_global(junction)) + ", dropping");
				junction_chain_invalid = true;
				break;
			}
			junctions.push_back(junction);
		}
		if(junction_chain_invalid){ ++dropped_count; continue; }

		// Resolve arc pair at each junction
		std::vector<ArcPairResult> junction_pairs;
		junction_pairs.reserve(junctions.size());
		bool chain_resolution_failed = false;
		for(std::size_t i = 0; i < junctions.size(); ++i){
			unsigned junction = junctions[i];
			auto from_candidates = find_incoming_arcs_of_way_at_node(g, way_chain[i], junction);
			auto to_candidates = find_outgoing_arcs_of_way_at_node(g, way_chain[i+1], junction);

			auto pair = resolve_unique_arc_pair(from_candidates, to_candidates);
			if(!pair.valid && i == 0)
				pair = disambiguate_arc_pair(g, from_candidates, to_candidates, junction, r.direction);

			if(!pair.valid){
				if(log_message)
					log_message("Via-way chain (relation " + std::to_string(r.osm_relation_id) + "): cannot resolve arcs at junction " + std::to_string(i) + ", dropping");
				chain_resolution_failed = true;
				break;
			}
			junction_pairs.push_back(pair);
		}
		if(chain_resolution_failed){ ++dropped_count; continue; }

		// Build full arc chain: [from_arc, intermediate_arcs..., to_arc]
		// For each junction i, junction_pairs[i].from_arc is the incoming arc and
		// junction_pairs[i].to_arc is the outgoing arc. Between consecutive junctions,
		// there may be additional arcs along the way (multi-arc segments).
		ViaWayChain chain;
		chain.mandatory = (r.category == OSMTurnRestrictionCategory::mandatory);

		// Start with from_arc at first junction
		chain.arcs.push_back(junction_pairs[0].from_arc);

		bool walk_failed = false;
		for(std::size_t i = 0; i < junction_pairs.size(); ++i){
			// Add the outgoing arc at this junction
			chain.arcs.push_back(junction_pairs[i].to_arc);

			// If there's a next junction, walk from this outgoing arc to the next junction's incoming arc
			if(i + 1 < junction_pairs.size()){
				unsigned next_junction = junctions[i + 1];
				unsigned walk_way = way_chain[i + 1]; // the way between junction i and junction i+1

				// Check if the outgoing arc already reaches the next junction
				if(g.head[junction_pairs[i].to_arc] != next_junction){
					auto intermediate = walk_way_arcs(g, junction_pairs[i].to_arc, next_junction, walk_way);
					if(intermediate.empty()){
						if(log_message)
							log_message("Via-way chain (relation " + std::to_string(r.osm_relation_id) + "): cannot walk way " + std::to_string(osm_way_chain[i+1]) + " between junctions, dropping");
						walk_failed = true;
						break;
					}
					for(unsigned arc : intermediate)
						chain.arcs.push_back(arc);
				}
			}
		}
		if(walk_failed){ ++dropped_count; continue; }

		if(log_message)
			log_message("Via-way chain (relation " + std::to_string(r.osm_relation_id) + "): resolved " + std::to_string(chain.arcs.size()) + " arcs, " + (chain.mandatory ? "mandatory" : "prohibitive"));

		chains.push_back(std::move(chain));
		++resolved_count;
	}

	if(log_message){
		log_message("Via-way chains: " + std::to_string(resolved_count) + " resolved, " + std::to_string(dropped_count) + " dropped");
	}

	return chains;
}

ResolvedConditionalTurns resolve_conditional_restrictions(
	const std::string& graph_dir,
	const std::string& pbf_file,
	const std::vector<RawConditionalRestriction>& raw_restrictions,
	std::function<bool(uint64_t, const TagMap&)>is_way_used,
	std::function<void(const std::string&)> log_message
){
	ResolvedConditionalTurns result;

	if(!is_way_used){
		if(log_message)
			log_message("No way-filter callback provided, falling back to car profile filter");
		is_way_used = [](uint64_t id, const TagMap&tags){ return is_osm_way_used_by_cars(id, tags); };
	}

	if(raw_restrictions.empty()){
		if(log_message) log_message("No conditional restrictions to resolve");
		return result;
	}

	// Load graph
	auto g = load_graph(graph_dir, log_message);

	// Rebuild ID mappings from PBF
	if(log_message) log_message("Rebuilding OSM ID mappings from PBF...");

	auto mapping = load_osm_id_mapping_from_pbf(
		pbf_file,
		[](uint64_t, const TagMap&){ return false; },
		[&](uint64_t osm_way_id, const TagMap& tags){
			return is_way_used(osm_way_id, tags);
		},
		log_message
	);

	IDMapper routing_node(mapping.is_routing_node);
	IDMapper routing_way(mapping.is_routing_way);

	if(log_message) log_message("Resolving " + std::to_string(raw_restrictions.size()) + " conditional restrictions...");

	unsigned resolved_via_node = 0;
	unsigned resolved_via_way = 0;
	unsigned dropped_count = 0;

	for(const auto& r : raw_restrictions){
		bool is_via_way = !r.via_ways.empty();

		// Map from_way and to_way to local IDs
		unsigned local_from_way = routing_way.to_local(r.from_way, invalid_id);
		unsigned local_to_way = routing_way.to_local(r.to_way, invalid_id);

		if(local_from_way == invalid_id || local_to_way == invalid_id){
			++dropped_count;
			continue;
		}

		if(!is_via_way){
			// --- Via-node restriction ---
			unsigned local_via_node = invalid_id;

			if(r.via_node != (uint64_t)-1){
				local_via_node = routing_node.to_local(r.via_node, invalid_id);
			}

			if(local_via_node == invalid_id){
				// Try to infer via_node: find unique intersection of from_way and to_way
				local_via_node = find_junction_node(g, local_from_way, local_to_way);
				if(local_via_node == invalid_id){
					if(log_message)
						log_message("Conditional restriction " + std::to_string(r.osm_relation_id) + ": cannot infer via node, dropping");
					++dropped_count;
					continue;
				}
			}

			auto from_candidates = find_incoming_arcs_of_way_at_node(g, local_from_way, local_via_node);
			auto to_candidates = find_outgoing_arcs_of_way_at_node(g, local_to_way, local_via_node);

			auto pair = disambiguate_arc_pair(g, from_candidates, to_candidates, local_via_node, r.direction);
			if(!pair.valid){
				if(log_message)
					log_message("Conditional restriction " + std::to_string(r.osm_relation_id) + ": cannot disambiguate arcs at via node, dropping");
				++dropped_count;
				continue;
			}

			if(r.category == OSMTurnRestrictionCategory::prohibitive){
				add_prohibitive_turn(pair.from_arc, pair.to_arc, r.condition_string,
					result.from_arc, result.to_arc, result.condition);
			} else {
				add_mandatory_turn(pair.from_arc, pair.to_arc, local_via_node, g, r.condition_string,
					result.from_arc, result.to_arc, result.condition);
			}
			++resolved_via_node;

		} else {
			// --- Via-way restriction (single or multi) ---
			std::vector<unsigned>way_chain;
			std::vector<uint64_t>osm_way_chain;
			way_chain.reserve(r.via_ways.size() + 2);
			osm_way_chain.reserve(r.via_ways.size() + 2);

			way_chain.push_back(local_from_way);
			osm_way_chain.push_back(r.from_way);
			for(uint64_t osm_via_way : r.via_ways){
				unsigned local_via_way = routing_way.to_local(osm_via_way, invalid_id);
				if(local_via_way == invalid_id){
					if(log_message)
						log_message("Conditional restriction " + std::to_string(r.osm_relation_id) + ": via-way " + std::to_string(osm_via_way) + " is not in the routing graph, dropping");
					++dropped_count;
					way_chain.clear();
					break;
				}
				way_chain.push_back(local_via_way);
				osm_way_chain.push_back(osm_via_way);
			}
			if(way_chain.empty())
				continue;
			way_chain.push_back(local_to_way);
			osm_way_chain.push_back(r.to_way);

			// Build junction chain between each consecutive pair of ways.
			std::vector<unsigned>junctions;
			junctions.reserve(way_chain.size() - 1);
			bool junction_chain_invalid = false;
			for(std::size_t i = 0; i + 1 < way_chain.size(); ++i){
				unsigned junction = find_junction_node(g, way_chain[i], way_chain[i+1]);
				if(junction == invalid_id){
					if(log_message){
						log_message(
							"Conditional restriction " + std::to_string(r.osm_relation_id) + ": cannot find unique junction between way "
							+ std::to_string(osm_way_chain[i]) + " and way " + std::to_string(osm_way_chain[i+1]) + ", dropping"
						);
					}
					junction_chain_invalid = true;
					break;
				}

				if(std::find(junctions.begin(), junctions.end(), junction) != junctions.end()){
					if(log_message)
						log_message("Conditional restriction " + std::to_string(r.osm_relation_id) + ": via-way chain reuses OSM junction node " + std::to_string(routing_node.to_global(junction)) + ", dropping");
					junction_chain_invalid = true;
					break;
				}

				junctions.push_back(junction);
			}
			if(junction_chain_invalid){
				++dropped_count;
				continue;
			}

			// Resolve one (incoming_arc, outgoing_arc) pair per chain junction.
			// The relation direction is only meaningful for the chain entry junction.
			std::vector<ArcPairResult>junction_pairs;
			junction_pairs.reserve(junctions.size());
			bool chain_resolution_failed = false;
			for(std::size_t i = 0; i < junctions.size(); ++i){
				unsigned junction = junctions[i];

				auto from_candidates = find_incoming_arcs_of_way_at_node(g, way_chain[i], junction);
				auto to_candidates = find_outgoing_arcs_of_way_at_node(g, way_chain[i+1], junction);

				auto pair = resolve_unique_arc_pair(from_candidates, to_candidates);
				if(!pair.valid && i == 0)
					pair = disambiguate_arc_pair(g, from_candidates, to_candidates, junction, r.direction);

				if(!pair.valid){
					if(log_message){
						if(i == 0){
							log_message("Conditional restriction " + std::to_string(r.osm_relation_id) + ": cannot resolve arcs at via-way chain entry junction, dropping");
						}else{
							log_message("Conditional restriction " + std::to_string(r.osm_relation_id) + ": cannot resolve arcs at via-way chain junction " + std::to_string(i) + ", dropping");
						}
					}
					chain_resolution_failed = true;
					break;
				}

				junction_pairs.push_back(pair);
			}
			if(chain_resolution_failed){
				++dropped_count;
				continue;
			}

			// Decompose into one local turn restriction per chain junction.
			if(r.category == OSMTurnRestrictionCategory::prohibitive){
				for(const auto&pair : junction_pairs){
					add_prohibitive_turn(
						pair.from_arc,
						pair.to_arc,
						r.condition_string,
						result.from_arc,
						result.to_arc,
						result.condition
					);
				}
			} else {
				for(std::size_t i = 0; i < junction_pairs.size(); ++i){
					add_mandatory_turn(
						junction_pairs[i].from_arc,
						junction_pairs[i].to_arc,
						junctions[i],
						g,
						r.condition_string,
						result.from_arc,
						result.to_arc,
						result.condition
					);
				}
			}
			++resolved_via_way;
		}
	}

	if(log_message){
		log_message("Resolved: " + std::to_string(resolved_via_node) + " via-node, "
			+ std::to_string(resolved_via_way) + " via-way, "
			+ std::to_string(dropped_count) + " dropped");
		log_message("Total turn pairs before dedup: " + std::to_string(result.from_arc.size()));
	}

	// Sort by (from_arc, to_arc) and deduplicate
	if(!result.from_arc.empty()){
		unsigned n = result.from_arc.size();

		// Build sort permutation
		std::vector<unsigned> perm(n);
		for(unsigned i = 0; i < n; ++i)
			perm[i] = i;
		std::sort(perm.begin(), perm.end(), [&](unsigned a, unsigned b){
			if(result.from_arc[a] != result.from_arc[b])
				return result.from_arc[a] < result.from_arc[b];
			if(result.to_arc[a] != result.to_arc[b])
				return result.to_arc[a] < result.to_arc[b];
			// Sort unconditional (empty condition) before conditional so dedup sees them first
			return result.condition[a].empty() > result.condition[b].empty();
		});

		// Apply permutation
		std::vector<unsigned> sorted_from(n), sorted_to(n);
		std::vector<std::string> sorted_cond(n);
		for(unsigned i = 0; i < n; ++i){
			sorted_from[i] = result.from_arc[perm[i]];
			sorted_to[i] = result.to_arc[perm[i]];
			sorted_cond[i] = std::move(result.condition[perm[i]]);
		}

		// Deduplicate: for identical (from, to), keep the one with the broadest condition.
		// If one is unconditional (empty string = always active), it subsumes conditional ones.
		// If both are conditional, keep both (they may have different time windows).
		std::vector<unsigned> dedup_from;
		std::vector<unsigned> dedup_to;
		std::vector<std::string> dedup_cond;

		for(unsigned i = 0; i < n; ++i){
			if(i > 0 && sorted_from[i] == sorted_from[i-1] && sorted_to[i] == sorted_to[i-1]){
				// Duplicate (from, to) pair
				if(sorted_cond[i].empty()){
					// Unconditional supersedes: replace the last entry
					dedup_cond.back() = "";
				} else if(!dedup_cond.back().empty()){
					// Both are conditional with different conditions — keep both
					dedup_from.push_back(sorted_from[i]);
					dedup_to.push_back(sorted_to[i]);
					dedup_cond.push_back(std::move(sorted_cond[i]));
				}
				// else: last is already unconditional, skip this conditional one
			} else {
				dedup_from.push_back(sorted_from[i]);
				dedup_to.push_back(sorted_to[i]);
				dedup_cond.push_back(std::move(sorted_cond[i]));
			}
		}

		result.from_arc = std::move(dedup_from);
		result.to_arc = std::move(dedup_to);
		result.condition = std::move(dedup_cond);
	}

	if(log_message)
		log_message("Final turn pairs after dedup: " + std::to_string(result.from_arc.size()));

	return result;
}

} // RoutingKit
