#include <routingkit/osm_condition_parser.h>

#include <cstring>
#include <cctype>

namespace RoutingKit{

namespace{

void skip_whitespace(const char*& p){
	while(*p == ' ' || *p == '\t')
		++p;
}

bool try_parse_day(const char*& p, int& day){
	static const char* day_names[] = {"Mo","Tu","We","Th","Fr","Sa","Su"};
	for(int i = 0; i < 7; ++i){
		if(p[0] == day_names[i][0] && p[1] == day_names[i][1]){
			day = i;
			p += 2;
			return true;
		}
	}
	return false;
}

bool try_parse_time(const char*& p, uint16_t& minutes){
	if(!isdigit(p[0]) || !isdigit(p[1]))
		return false;
	unsigned h = (p[0]-'0')*10 + (p[1]-'0');
	if(p[2] != ':')
		return false;
	if(!isdigit(p[3]) || !isdigit(p[4]))
		return false;
	unsigned m = (p[3]-'0')*10 + (p[4]-'0');
	if(h > 24 || (h == 24 && m != 0) || m > 59)
		return false;
	minutes = (uint16_t)(h*60 + m);
	p += 5;
	return true;
}

// Parses day specifiers into a bitmask.
// Handles: "Mo-Fr", "Sa,Su", "Mo,We,Fr", "Mo-Su"
// Returns 0 on failure. Sets p past the parsed days.
uint8_t parse_day_spec(const char*& p){
	uint8_t mask = 0;

	int first_day;
	if(!try_parse_day(p, first_day))
		return 0;

	if(*p == '-'){
		++p;
		int last_day;
		if(!try_parse_day(p, last_day))
			return 0;
		// Handle wrapping ranges like Fr-Mo (4→0 = Fri,Sat,Sun,Mon)
		for(int d = first_day; d != (last_day + 1) % 7; d = (d + 1) % 7)
			mask |= (1 << d);
	} else {
		mask |= (1 << first_day);
	}

	while(*p == ','){
		const char* before_comma = p;
		++p;
		skip_whitespace(p);

		int day;
		if(!try_parse_day(p, day)){
			p = before_comma;
			break;
		}

		if(*p == '-'){
			++p;
			int last_day;
			if(!try_parse_day(p, last_day)){
				p = before_comma;
				break;
			}
			for(int d = day; d != (last_day + 1) % 7; d = (d + 1) % 7)
				mask |= (1 << d);
		} else {
			mask |= (1 << day);
		}
	}

	return mask;
}

// Parses one or more time ranges: "07:00-09:00" or "07:00-09:00,16:00-18:00"
// Returns windows with the given day_mask applied.
bool parse_time_ranges(const char*& p, uint8_t day_mask, std::vector<TimeWindow>& out){
	uint16_t start, end;
	if(!try_parse_time(p, start))
		return false;
	if(*p != '-')
		return false;
	++p;
	if(!try_parse_time(p, end))
		return false;
	out.push_back({day_mask, start, end});

	while(*p == ','){
		const char* saved = p;
		++p;
		skip_whitespace(p);
		if(!try_parse_time(p, start)){
			p = saved;
			break;
		}
		if(*p != '-'){
			p = saved;
			break;
		}
		++p;
		if(!try_parse_time(p, end)){
			p = saved;
			break;
		}
		out.push_back({day_mask, start, end});
	}
	return true;
}

// Parses a single condition block (content inside parentheses), e.g.:
//   "Mo-Fr 07:00-09:00"
//   "Sa,Su 10:00-14:00"
//   "06:00-22:00"              (no day spec → all days)
//   "Mo-Fr 07:00-09:00,16:00-18:00"
bool parse_condition_block(const char*& p, std::vector<TimeWindow>& out){
	skip_whitespace(p);

	// Check for PH (public holiday) — deferred, skip gracefully
	if(p[0] == 'P' && p[1] == 'H'){
		return false;
	}

	// Try parsing day spec first
	const char* saved = p;
	uint8_t day_mask = parse_day_spec(p);

	if(day_mask != 0){
		skip_whitespace(p);
		// If next char is a digit, parse time ranges
		if(isdigit(*p)){
			return parse_time_ranges(p, day_mask, out);
		}
		// Day-only condition (e.g. "Sa,Su") — active all day
		out.push_back({day_mask, 0, 1440});
		return true;
	}

	// No day spec found — try time-only (applies to all days)
	p = saved;
	if(isdigit(*p)){
		return parse_time_ranges(p, 0x7F, out);
	}

	return false;
}

} // anonymous namespace

std::vector<ParsedCondition> parse_conditional_value(const char* conditional_value){
	std::vector<ParsedCondition> results;
	if(!conditional_value || *conditional_value == '\0')
		return results;

	const char* p = conditional_value;

	while(*p != '\0'){
		skip_whitespace(p);
		if(*p == '\0')
			break;

		// Parse restriction value (everything before " @ ")
		const char* value_start = p;
		const char* at_pos = strstr(p, " @ ");
		if(!at_pos)
			break;

		std::string restriction_value(value_start, at_pos);

		p = at_pos + 3; // skip " @ "
		skip_whitespace(p);

		// Expect opening parenthesis
		if(*p != '(')
			break;
		++p;

		ParsedCondition cond;
		cond.restriction_value = std::move(restriction_value);

		// Parse condition blocks separated by ';'
		bool ok = true;
		while(ok){
			skip_whitespace(p);
			if(*p == ')' || *p == '\0')
				break;

			ok = parse_condition_block(p, cond.time_windows);

			skip_whitespace(p);
			if(*p == ';'){
				++p;
				ok = true; // continue to next block
			}
		}

		// Skip to closing parenthesis
		while(*p != ')' && *p != '\0')
			++p;
		if(*p == ')')
			++p;

		if(!cond.time_windows.empty())
			results.push_back(std::move(cond));

		// Skip separator between multiple conditions (';' at top level)
		skip_whitespace(p);
		if(*p == ';')
			++p;
	}

	return results;
}

bool is_time_window_active(
	const std::vector<TimeWindow>& windows,
	unsigned day_of_week,
	unsigned minutes_since_midnight
){
	if(day_of_week > 6 || minutes_since_midnight >= 1440)
		return false;

	uint8_t day_bit = (uint8_t)(1u << day_of_week);

	for(const auto& w : windows){
		if(!(w.day_mask & day_bit))
			continue;

		if(w.end_minutes > w.start_minutes){
			// Normal range
			if(minutes_since_midnight >= w.start_minutes && minutes_since_midnight < w.end_minutes)
				return true;
		} else if(w.end_minutes < w.start_minutes){
			// Wraps midnight: active if >= start OR < end
			if(minutes_since_midnight >= w.start_minutes || minutes_since_midnight < w.end_minutes)
				return true;
		} else {
			// start == end → full day
			return true;
		}
	}

	return false;
}

} // RoutingKit
