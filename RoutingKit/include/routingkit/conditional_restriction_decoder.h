#ifndef ROUTING_KIT_CONDITIONAL_RESTRICTION_DECODER_H
#define ROUTING_KIT_CONDITIONAL_RESTRICTION_DECODER_H

#include <routingkit/osm_graph_builder.h>

#include <vector>
#include <functional>
#include <string>
#include <cstdint>

namespace RoutingKit{

struct RawConditionalRestriction{
	uint64_t osm_relation_id;

	OSMTurnRestrictionCategory category;
	OSMTurnDirection direction;

	uint64_t from_way;
	uint64_t to_way;

	uint64_t via_node;
	std::vector<uint64_t> via_ways;

	std::string condition_string;
};

struct ConditionalTagPriority{
	const char*primary_conditional;
	const char*fallback_conditional;
	const char*primary_unconditional;
	const char*fallback_unconditional;
};

inline ConditionalTagPriority car_conditional_tag_priority(){
	return {
		"restriction:motorcar:conditional",
		"restriction:conditional",
		"restriction:motorcar",
		"restriction"
	};
}

inline ConditionalTagPriority motorcycle_conditional_tag_priority(){
	return {
		"restriction:motorcycle:conditional",
		"restriction:conditional",
		"restriction:motorcycle",
		"restriction"
	};
}

void scan_conditional_restrictions_from_pbf(
	const std::string& pbf_file,
	std::function<void(RawConditionalRestriction)> on_restriction,
	ConditionalTagPriority tag_priority,
	std::function<void(const std::string&)> log_message = nullptr
);

inline void scan_conditional_restrictions_from_pbf(
	const std::string& pbf_file,
	std::function<void(RawConditionalRestriction)> on_restriction,
	std::function<void(const std::string&)> log_message = nullptr
){
	scan_conditional_restrictions_from_pbf(
		pbf_file,
		on_restriction,
		car_conditional_tag_priority(),
		log_message
	);
}

} // RoutingKit

#endif
