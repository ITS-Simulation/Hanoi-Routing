#ifndef ROUTING_KIT_OSM_CONDITION_PARSER_H
#define ROUTING_KIT_OSM_CONDITION_PARSER_H

#include <vector>
#include <string>
#include <cstdint>

namespace RoutingKit{

struct TimeWindow{
	uint8_t day_mask;
	uint16_t start_minutes;
	uint16_t end_minutes;
};

struct ParsedCondition{
	std::string restriction_value;
	std::vector<TimeWindow> time_windows;
};

std::vector<ParsedCondition> parse_conditional_value(const char* conditional_value);

bool is_time_window_active(
	const std::vector<TimeWindow>& windows,
	unsigned day_of_week,
	unsigned minutes_since_midnight
);

} // RoutingKit

#endif
