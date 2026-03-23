#include <routingkit/conditional_restriction_decoder.h>
#include <routingkit/osm_decoder.h>
#include <routingkit/tag_map.h>

#include <cstring>
#include <string>

namespace RoutingKit{

namespace{

bool str_eq(const char* l, const char* r){
	return !strcmp(l, r);
}

bool starts_with(const char* prefix, const char* str){
	while(*prefix != '\0' && *str == *prefix){
		++prefix;
		++str;
	}
	return *prefix == '\0';
}

struct ParsedRestrictionValue{
	bool valid;
	OSMTurnRestrictionCategory category;
	OSMTurnDirection direction;
};

ParsedRestrictionValue parse_restriction_value(const char* value){
	ParsedRestrictionValue result;
	result.valid = false;

	int direction_offset;

	if(starts_with("only_", value)){
		result.category = OSMTurnRestrictionCategory::mandatory;
		direction_offset = 5;
	} else if(starts_with("no_", value)){
		result.category = OSMTurnRestrictionCategory::prohibitive;
		direction_offset = 3;
	} else {
		return result;
	}

	if(str_eq("left_turn", value+direction_offset)){
		result.direction = OSMTurnDirection::left_turn;
	} else if(str_eq("right_turn", value+direction_offset)){
		result.direction = OSMTurnDirection::right_turn;
	} else if(str_eq("straight_on", value+direction_offset)){
		result.direction = OSMTurnDirection::straight_on;
	} else if(str_eq("u_turn", value+direction_offset)){
		result.direction = OSMTurnDirection::u_turn;
	} else {
		return result;
	}

	result.valid = true;
	return result;
}

struct ParsedMembers{
	bool valid;
	std::vector<uint64_t> from_ways;
	std::vector<uint64_t> to_ways;
	uint64_t via_node;
	std::vector<uint64_t> via_ways;
	bool has_via_relation;
};

ParsedMembers parse_relation_members(
	uint64_t osm_relation_id,
	const std::vector<OSMRelationMember>& member_list,
	std::function<void(const std::string&)> log_message
){
	ParsedMembers result;
	result.valid = true;
	result.via_node = (uint64_t)-1;
	result.has_via_relation = false;

	for(unsigned i = 0; i < member_list.size(); ++i){
		if(str_eq(member_list[i].role, "from")){
			if(member_list[i].type == OSMIDType::way){
				result.from_ways.push_back(member_list[i].id);
			} else {
				if(log_message)
					log_message("Conditional restriction " + std::to_string(osm_relation_id) + ": \"from\" member is not a way, skipping member");
			}
		} else if(str_eq(member_list[i].role, "to")){
			if(member_list[i].type == OSMIDType::way){
				result.to_ways.push_back(member_list[i].id);
			} else {
				if(log_message)
					log_message("Conditional restriction " + std::to_string(osm_relation_id) + ": \"to\" member is not a way, skipping member");
			}
		} else if(str_eq(member_list[i].role, "via")){
			if(member_list[i].type == OSMIDType::relation){
				result.has_via_relation = true;
			} else if(member_list[i].type == OSMIDType::node){
				result.via_node = member_list[i].id;
			} else if(member_list[i].type == OSMIDType::way){
				result.via_ways.push_back(member_list[i].id);
			}
		} else if(!str_eq(member_list[i].role, "location_hint")){
			if(log_message)
				log_message("Conditional restriction " + std::to_string(osm_relation_id) + ": unknown role \"" + member_list[i].role + "\"");
		}
	}

	if(result.has_via_relation){
		if(log_message)
			log_message("Conditional restriction " + std::to_string(osm_relation_id) + ": via member is a relation, skipping");
		result.valid = false;
		return result;
	}

	// Reject having both via-node and via-way — invalid OSM structure
	if(result.via_node != (uint64_t)-1 && !result.via_ways.empty()){
		if(log_message)
			log_message("Conditional restriction " + std::to_string(osm_relation_id) + ": has both via-node and via-way, skipping");
		result.valid = false;
		return result;
	}

	if(result.from_ways.empty()){
		if(log_message)
			log_message("Conditional restriction " + std::to_string(osm_relation_id) + ": missing \"from\" role");
		result.valid = false;
		return result;
	}

	if(result.to_ways.empty()){
		if(log_message)
			log_message("Conditional restriction " + std::to_string(osm_relation_id) + ": missing \"to\" role");
		result.valid = false;
		return result;
	}

	return result;
}

// Extracts the restriction value from a conditional tag value.
// Format: "no_right_turn @ (Mo-Fr 07:00-09:00)"
// Returns the part before " @ " as the restriction value.
const char* extract_restriction_from_conditional(const char* cond_value, std::string& restriction_out, std::string& condition_out){
	const char* at_pos = strstr(cond_value, " @ ");
	if(!at_pos)
		return nullptr;

	restriction_out.assign(cond_value, at_pos);

	const char* cond_start = at_pos + 3;
	while(*cond_start == ' ') ++cond_start;

	// Strip parentheses if present
	if(*cond_start == '('){
		++cond_start;
		const char* cond_end = cond_start;
		int depth = 1;
		while(*cond_end != '\0' && depth > 0){
			if(*cond_end == '(') ++depth;
			else if(*cond_end == ')') --depth;
			++cond_end;
		}
		if(depth == 0)
			--cond_end;
		condition_out.assign(cond_start, cond_end);
	} else {
		condition_out = cond_start;
	}

	return cond_value;
}

} // anonymous namespace

void scan_conditional_restrictions_from_pbf(
	const std::string& pbf_file,
	std::function<void(RawConditionalRestriction)> on_restriction,
	ConditionalTagPriority tag_priority,
	std::function<void(const std::string&)> log_message
){
	unsigned conditional_count = 0;
	unsigned via_way_count = 0;
	unsigned skipped_count = 0;

	if(!tag_priority.primary_conditional ||
	   !tag_priority.fallback_conditional ||
	   !tag_priority.primary_unconditional ||
	   !tag_priority.fallback_unconditional){
		if(log_message)
			log_message("Incomplete conditional tag priority configuration, falling back to car tag priority");
		tag_priority = car_conditional_tag_priority();
	}

	unordered_read_osm_pbf(
		pbf_file,
		nullptr, // node callback — not needed
		nullptr, // way callback — not needed
		[&](uint64_t osm_relation_id, const std::vector<OSMRelationMember>& member_list, const TagMap& tags){

			const char* type_tag = tags["type"];
			if(!type_tag)
				return;
			if(!str_eq(type_tag, "restriction") &&
			   !starts_with("restriction:", type_tag))
				return;

			// Check for conditional restriction tags
			const char* conditional_tag = tags[tag_priority.primary_conditional];
			if(!conditional_tag)
				conditional_tag = tags[tag_priority.fallback_conditional];

			// Check for unconditional restriction tag (for via-way detection)
			const char* unconditional_tag = tags[tag_priority.primary_unconditional];
			if(!unconditional_tag)
				unconditional_tag = tags[tag_priority.fallback_unconditional];

			if(!conditional_tag && !unconditional_tag)
				return;

			auto members = parse_relation_members(osm_relation_id, member_list, log_message);
			if(!members.valid){
				++skipped_count;
				return;
			}

			bool is_via_way = !members.via_ways.empty();
			bool is_via_node = (members.via_node != (uint64_t)-1);

			// Process conditional restrictions (via-node or via-way)
			if(conditional_tag){
				std::string restriction_value, condition_string;
				if(!extract_restriction_from_conditional(conditional_tag, restriction_value, condition_string)){
					if(log_message)
						log_message("Conditional restriction " + std::to_string(osm_relation_id) + ": failed to parse conditional tag value");
					++skipped_count;
					return;
				}

				auto parsed = parse_restriction_value(restriction_value.c_str());
				if(!parsed.valid){
					if(log_message)
						log_message("Conditional restriction " + std::to_string(osm_relation_id) + ": unknown restriction value \"" + restriction_value + "\"");
					++skipped_count;
					return;
				}

				// Mandatory restrictions with multiple from/to are invalid
				if(parsed.category == OSMTurnRestrictionCategory::mandatory){
					if(members.from_ways.size() != 1 || members.to_ways.size() != 1){
						if(log_message)
							log_message("Conditional restriction " + std::to_string(osm_relation_id) + ": mandatory with multiple from/to, skipping");
						++skipped_count;
						return;
					}
				}

				for(auto from_way : members.from_ways){
					for(auto to_way : members.to_ways){
						RawConditionalRestriction r;
						r.osm_relation_id = osm_relation_id;
						r.category = parsed.category;
						r.direction = parsed.direction;
						r.from_way = from_way;
						r.to_way = to_way;
						r.via_node = members.via_node;
						r.via_ways = members.via_ways;
						r.condition_string = condition_string;
						on_restriction(std::move(r));
					}
				}
				++conditional_count;
				// NOTE: if a relation carries both a conditional tag and an unconditional
				// via-way tag, we return here and the unconditional path below is never
				// reached. This is intentional: the conditional restriction supersedes the
				// unconditional one for this profile.
				return;
			}

			// Process unconditional via-way restrictions (those RoutingKit currently drops)
			if(unconditional_tag && is_via_way && !is_via_node){
				auto parsed = parse_restriction_value(unconditional_tag);
				if(!parsed.valid){
					++skipped_count;
					return;
				}

				if(parsed.category == OSMTurnRestrictionCategory::mandatory){
					if(members.from_ways.size() != 1 || members.to_ways.size() != 1){
						++skipped_count;
						return;
					}
				}

				for(auto from_way : members.from_ways){
					for(auto to_way : members.to_ways){
						RawConditionalRestriction r;
						r.osm_relation_id = osm_relation_id;
						r.category = parsed.category;
						r.direction = parsed.direction;
						r.from_way = from_way;
						r.to_way = to_way;
						r.via_node = (uint64_t)-1;
						r.via_ways = members.via_ways;
						r.condition_string = ""; // unconditional — always active
						on_restriction(std::move(r));
					}
				}
				++via_way_count;
				return;
			}

			// Unconditional via-node restrictions are already handled by RoutingKit's
			// existing decoder — skip them here.
		},
		log_message
	);

	if(log_message){
		log_message("Scanned PBF: " + std::to_string(conditional_count) + " conditional restrictions, "
			+ std::to_string(via_way_count) + " unconditional via-way restrictions, "
			+ std::to_string(skipped_count) + " skipped");
	}
}

} // RoutingKit
