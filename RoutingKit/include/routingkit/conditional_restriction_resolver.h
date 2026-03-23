#ifndef ROUTING_KIT_CONDITIONAL_RESTRICTION_RESOLVER_H
#define ROUTING_KIT_CONDITIONAL_RESTRICTION_RESOLVER_H

#include <routingkit/conditional_restriction_decoder.h>
#include <routingkit/osm_profile.h>

#include <vector>
#include <functional>
#include <string>

namespace RoutingKit{

struct ResolvedConditionalTurns{
	std::vector<unsigned> from_arc;
	std::vector<unsigned> to_arc;
	std::vector<std::string> condition;
};

struct ViaWayChain{
	std::vector<unsigned> arcs;  // [from_arc, v1, ..., vN, to_arc]
	bool mandatory;              // false = prohibitive, true = mandatory (only_*)
};

ResolvedConditionalTurns resolve_conditional_restrictions(
	const std::string& graph_dir,
	const std::string& pbf_file,
	const std::vector<RawConditionalRestriction>& raw_restrictions,
	std::function<bool(uint64_t, const TagMap&)>is_way_used,
	std::function<void(const std::string&)> log_message = nullptr
);

std::vector<ViaWayChain> resolve_via_way_chains(
	const std::string& graph_dir,
	const std::string& pbf_file,
	const std::vector<RawConditionalRestriction>& raw_restrictions,
	std::function<bool(uint64_t, const TagMap&)>is_way_used,
	std::function<void(const std::string&)> log_message = nullptr
);

inline std::vector<ViaWayChain> resolve_via_way_chains(
	const std::string& graph_dir,
	const std::string& pbf_file,
	const std::vector<RawConditionalRestriction>& raw_restrictions,
	std::function<void(const std::string&)>log_message = nullptr
){
	return resolve_via_way_chains(
		graph_dir,
		pbf_file,
		raw_restrictions,
		[](uint64_t id, const TagMap&tags){ return is_osm_way_used_by_cars(id, tags); },
		log_message
	);
}

inline ResolvedConditionalTurns resolve_conditional_restrictions(
	const std::string& graph_dir,
	const std::string& pbf_file,
	const std::vector<RawConditionalRestriction>& raw_restrictions,
	std::function<void(const std::string&)>log_message = nullptr
){
	return resolve_conditional_restrictions(
		graph_dir,
		pbf_file,
		raw_restrictions,
		[](uint64_t id, const TagMap&tags){ return is_osm_way_used_by_cars(id, tags); },
		log_message
	);
}

} // RoutingKit

#endif
