#include <routingkit/osm_condition_parser.h>
#include <routingkit/conditional_restriction_decoder.h>
#include <routingkit/conditional_restriction_resolver.h>
#include <routingkit/vector_io.h>
#include <routingkit/timer.h>

#include <iostream>
#include <filesystem>
#include <fstream>
#include <string>
#include <system_error>
#include <vector>
#include <unordered_set>
#include <cstring>
#include <cstdint>

using namespace RoutingKit;

namespace{

void save_unsigned_vector(const std::string& file_name, const std::vector<unsigned>& vec){
	std::ofstream out(file_name, std::ios::binary);
	if(!out)
		throw std::runtime_error("Cannot open \"" + file_name + "\" for writing.");

	if(!vec.empty())
		out.write(reinterpret_cast<const char*>(vec.data()), vec.size() * sizeof(unsigned));

	if(!out)
		throw std::runtime_error("Error writing to \"" + file_name + "\".");
}

void save_uint8_vector(const std::string& file_name, const std::vector<uint8_t>& vec){
	std::ofstream out(file_name, std::ios::binary);
	if(!out)
		throw std::runtime_error("Cannot open \"" + file_name + "\" for writing.");

	if(!vec.empty())
		out.write(reinterpret_cast<const char*>(vec.data()), vec.size() * sizeof(uint8_t));

	if(!out)
		throw std::runtime_error("Error writing to \"" + file_name + "\".");
}

void save_time_windows(const std::string& file_name, const std::vector<std::vector<TimeWindow>>& windows){
	unsigned n = windows.size();

	// Build offset array
	std::vector<uint32_t> offsets(n + 1);
	offsets[0] = 0;
	for(unsigned i = 0; i < n; ++i)
		offsets[i+1] = offsets[i] + (uint32_t)windows[i].size();

	std::ofstream out(file_name, std::ios::binary);
	if(!out)
		throw std::runtime_error("Cannot open \"" + file_name + "\" for writing.");

	// Write offsets
	out.write(reinterpret_cast<const char*>(offsets.data()), (n+1) * sizeof(uint32_t));

	// Write packed TimeWindow structs (5 bytes each: 1 + 2 + 2)
	for(unsigned i = 0; i < n; ++i){
		for(const auto& tw : windows[i]){
			out.write(reinterpret_cast<const char*>(&tw.day_mask), sizeof(tw.day_mask));
			out.write(reinterpret_cast<const char*>(&tw.start_minutes), sizeof(tw.start_minutes));
			out.write(reinterpret_cast<const char*>(&tw.end_minutes), sizeof(tw.end_minutes));
		}
	}

	if(!out)
		throw std::runtime_error("Error writing to \"" + file_name + "\".");
}

void print_usage(const char* program_name){
	std::cerr << "Usage: " << program_name << " <pbf_file> <graph_dir> [<output_dir>] [--profile car|motorcycle]" << std::endl;
	std::cerr << std::endl;
	std::cerr << "Extracts conditional turn restrictions and via-way restrictions from a PBF file" << std::endl;
	std::cerr << "using an already-extracted graph. Writes output binary files compatible with" << std::endl;
	std::cerr << "RoutingKit's forbidden_turn_from_arc/forbidden_turn_to_arc format." << std::endl;
	std::cerr << std::endl;
	std::cerr << "  <pbf_file>    Path to the .osm.pbf file" << std::endl;
	std::cerr << "  <graph_dir>   Directory containing first_out, head, way, latitude, longitude" << std::endl;
	std::cerr << "  <output_dir>  Profile output root (defaults to graph_dir); files are written to" << std::endl;
	std::cerr << "                <output_dir>/conditional_turns/" << std::endl;
	std::cerr << "  --profile     Routing profile: car (default) or motorcycle" << std::endl;
}

std::string get_conditional_output_dir(const std::string&output_root_dir){
	std::filesystem::path conditional_dir = std::filesystem::path(output_root_dir) / "conditional_turns";
	std::error_code ec;
	std::filesystem::create_directories(conditional_dir, ec);
	if(ec)
		throw std::runtime_error("Cannot create output directory \"" + conditional_dir.string() + "\": " + ec.message());
	return conditional_dir.string();
}

} // anonymous namespace

int main(int argc, char* argv[]){
	if(argc < 3){
		print_usage(argv[0]);
		return 1;
	}

	std::string pbf_file = argv[1];
	std::string graph_dir = argv[2];
	std::string output_dir = graph_dir;
	std::string profile = "car";
	bool output_dir_set = false;

	for(int i = 3; i < argc; ++i){
		std::string arg = argv[i];
		if(arg == "--profile"){
			if(i+1 >= argc){
				std::cerr << "Error: --profile requires a value (car|motorcycle)" << std::endl;
				print_usage(argv[0]);
				return 1;
			}
			profile = argv[++i];
		}else if(arg.rfind("--profile=", 0) == 0){
			profile = arg.substr(std::strlen("--profile="));
		}else if(!arg.empty() && arg[0] == '-'){
			std::cerr << "Error: unknown option \"" << arg << "\"" << std::endl;
			print_usage(argv[0]);
			return 1;
		}else if(!output_dir_set){
			output_dir = arg;
			output_dir_set = true;
		}else{
			std::cerr << "Error: unexpected extra positional argument \"" << arg << "\"" << std::endl;
			print_usage(argv[0]);
			return 1;
		}
	}

	if(profile != "car" && profile != "motorcycle"){
		std::cerr << "Error: unsupported profile \"" << profile << "\" (expected car|motorcycle)" << std::endl;
		print_usage(argv[0]);
		return 1;
	}

	ConditionalTagPriority tag_priority = car_conditional_tag_priority();
	std::function<bool(uint64_t, const TagMap&)> is_way_used = [](uint64_t id, const TagMap&tags){
		return is_osm_way_used_by_cars(id, tags);
	};
	if(profile == "motorcycle"){
		tag_priority = motorcycle_conditional_tag_priority();
		is_way_used = [](uint64_t id, const TagMap&tags){
			return is_osm_way_used_by_motorcycles(id, tags);
		};
	}

	auto log = [](const std::string& msg){
		std::cout << msg << std::endl;
	};

	long long timer;

	// Step 1: Scan PBF for conditional + via-way restrictions
	log("=== Step 1: Scanning PBF for conditional and via-way restrictions ===");
	log("Profile: " + profile);
	timer = -get_micro_time();

	std::vector<RawConditionalRestriction> raw;
	scan_conditional_restrictions_from_pbf(
		pbf_file,
		[&](auto r){ raw.push_back(std::move(r)); },
		tag_priority,
		log
	);

	timer += get_micro_time();
	log("PBF scan completed in " + std::to_string(timer) + " μs, found " + std::to_string(raw.size()) + " raw restrictions");

	if(raw.empty()){
		log("No conditional or via-way restrictions found. Writing empty output files.");
		const std::string conditional_output_dir = get_conditional_output_dir(output_dir);
		save_unsigned_vector(conditional_output_dir + "/conditional_turn_from_arc", std::vector<unsigned>{});
		save_unsigned_vector(conditional_output_dir + "/conditional_turn_to_arc", std::vector<unsigned>{});
		save_time_windows(conditional_output_dir + "/conditional_turn_time_windows", {});

		// Write empty via-way chain files (mandatory input for generate_line_graph)
		save_unsigned_vector(graph_dir + "/via_way_chain_offsets", std::vector<unsigned>{0});
		save_unsigned_vector(graph_dir + "/via_way_chain_arcs", std::vector<unsigned>{});
		save_uint8_vector(graph_dir + "/via_way_chain_mandatory", std::vector<uint8_t>{});
		log("Empty via-way chain files written to " + graph_dir);

		return 0;
	}

	// Step 2: Resolve to arc pairs
	log("=== Step 2: Resolving restrictions to arc pairs ===");
	timer = -get_micro_time();

	auto resolved = resolve_conditional_restrictions(graph_dir, pbf_file, raw, is_way_used, log);

	timer += get_micro_time();
	log("Resolution completed in " + std::to_string(timer) + " μs, " + std::to_string(resolved.from_arc.size()) + " turn pairs");

	// Step 2a: Resolve unconditional via-way restrictions into full arc chains
	log("=== Step 2a: Resolving unconditional via-way restrictions into arc chains ===");
	timer = -get_micro_time();

	auto via_way_chains = resolve_via_way_chains(graph_dir, pbf_file, raw, is_way_used, log);

	timer += get_micro_time();
	log("Via-way chain resolution completed in " + std::to_string(timer) + " μs, " + std::to_string(via_way_chains.size()) + " chains");

	// Write via-way chain files to graph_dir (mandatory input for generate_line_graph)
	{
		std::vector<unsigned> chain_offsets;
		std::vector<unsigned> chain_arcs;
		std::vector<uint8_t> chain_mandatory;

		chain_offsets.reserve(via_way_chains.size() + 1);
		chain_offsets.push_back(0);

		for(const auto& chain : via_way_chains){
			chain_arcs.insert(chain_arcs.end(), chain.arcs.begin(), chain.arcs.end());
			chain_offsets.push_back((unsigned)chain_arcs.size());
			chain_mandatory.push_back(chain.mandatory ? 1 : 0);
		}

		save_unsigned_vector(graph_dir + "/via_way_chain_offsets", chain_offsets);
		save_unsigned_vector(graph_dir + "/via_way_chain_arcs", chain_arcs);
		save_uint8_vector(graph_dir + "/via_way_chain_mandatory", chain_mandatory);

		log("Via-way chain files written to " + graph_dir);
		log("  via_way_chain_offsets: " + std::to_string(chain_offsets.size()) + " entries (" + std::to_string(via_way_chains.size()) + " chains)");
		log("  via_way_chain_arcs: " + std::to_string(chain_arcs.size()) + " arcs total");
		log("  via_way_chain_mandatory: " + std::to_string(chain_mandatory.size()) + " entries");
	}

	// Step 2b: Remove conditional turns that overlap with unconditional forbidden turns
	log("=== Step 2b: Filtering out overlaps with forbidden turns ===");
	timer = -get_micro_time();
	unsigned overlap_count = 0;
	bool overlap_filter_applied = false;
	[&]{
		std::vector<unsigned> forbidden_from, forbidden_to;
		try{
			forbidden_from = load_vector<unsigned>(graph_dir + "/forbidden_turn_from_arc");
			forbidden_to = load_vector<unsigned>(graph_dir + "/forbidden_turn_to_arc");
		}catch(const std::exception& e){
			log("Warning: could not load forbidden turn files (" + std::string(e.what()) + "), skipping overlap filter");
			log("Overlap filter skipped: forbidden turn files are unavailable");
			return;
		}

		if(forbidden_from.size() != forbidden_to.size()){
			log("Warning: forbidden turn vectors have mismatched sizes, skipping overlap filter");
			log("Overlap filter skipped: forbidden turn vectors have mismatched lengths");
			return;
		}

		std::unordered_set<uint64_t> forbidden_set;
		forbidden_set.reserve(forbidden_from.size());
		for(std::size_t i = 0; i < forbidden_from.size(); ++i)
			forbidden_set.insert(((uint64_t)forbidden_from[i] << 32) | (uint64_t)forbidden_to[i]);

		std::vector<unsigned> filtered_from, filtered_to;
		std::vector<std::string> filtered_condition;

		for(std::size_t i = 0; i < resolved.from_arc.size(); ++i){
			uint64_t key = ((uint64_t)resolved.from_arc[i] << 32) | (uint64_t)resolved.to_arc[i];
			if(forbidden_set.count(key)){
				++overlap_count;
			}else{
				filtered_from.push_back(resolved.from_arc[i]);
				filtered_to.push_back(resolved.to_arc[i]);
				filtered_condition.push_back(std::move(resolved.condition[i]));
			}
		}

		resolved.from_arc = std::move(filtered_from);
		resolved.to_arc = std::move(filtered_to);
		resolved.condition = std::move(filtered_condition);
		overlap_filter_applied = true;
	}();
	timer += get_micro_time();
	if(overlap_filter_applied){
		log("Overlap filter completed in " + std::to_string(timer) + " μs: removed "
			+ std::to_string(overlap_count) + " conditional turns that duplicate forbidden turns, "
			+ std::to_string(resolved.from_arc.size()) + " remaining");
	}else{
		log("Overlap filter completed in " + std::to_string(timer) + " μs (skipped)");
	}

	// Step 3: Parse condition strings into TimeWindows
	log("=== Step 3: Parsing condition strings into time windows ===");
	timer = -get_micro_time();

	unsigned n = resolved.condition.size();
	std::vector<std::vector<TimeWindow>> parsed_windows(n);
	unsigned unconditional_count = 0;
	unsigned parse_failure_count = 0;

	for(unsigned i = 0; i < n; ++i){
		if(resolved.condition[i].empty()){
			++unconditional_count;
			// Unconditional restrictions must always be active: assign all-day-every-day window
			parsed_windows[i].push_back({0x7F, 0, 1440});
			continue;
		}

		// The condition string is the raw content inside parentheses.
		// Wrap it as "no_turn @ (<condition>)" so parse_conditional_value can parse it.
		std::string synthetic = "no_turn @ (" + resolved.condition[i] + ")";
		auto parsed = parse_conditional_value(synthetic.c_str());
		if(parsed.empty()){
			log("Warning: failed to parse condition \"" + resolved.condition[i] + "\" for pair ("
				+ std::to_string(resolved.from_arc[i]) + ", " + std::to_string(resolved.to_arc[i]) + "), treating as always-active");
			++parse_failure_count;
			// Conservative fallback: unparsable condition is treated as always-active
			// so the restriction is enforced at all times rather than silently dropped.
			parsed_windows[i].push_back({0x7F, 0, 1440});
			continue;
		}
		for(auto& pc : parsed)
			parsed_windows[i].insert(parsed_windows[i].end(), pc.time_windows.begin(), pc.time_windows.end());
	}

	timer += get_micro_time();
	log("Parsing completed in " + std::to_string(timer) + " μs: "
		+ std::to_string(unconditional_count) + " unconditional, "
		+ std::to_string(n - unconditional_count - parse_failure_count) + " conditional with time windows, "
		+ std::to_string(parse_failure_count) + " parse failures");

	// Step 4: Save output
	log("=== Step 4: Writing output files ===");
	timer = -get_micro_time();

	const std::string conditional_output_dir = get_conditional_output_dir(output_dir);
	save_unsigned_vector(conditional_output_dir + "/conditional_turn_from_arc", resolved.from_arc);
	save_unsigned_vector(conditional_output_dir + "/conditional_turn_to_arc", resolved.to_arc);
	save_time_windows(conditional_output_dir + "/conditional_turn_time_windows", parsed_windows);

	timer += get_micro_time();
	log("Output written in " + std::to_string(timer) + " μs to " + conditional_output_dir);
	log("  conditional_turn_from_arc: " + std::to_string(resolved.from_arc.size()) + " entries");
	log("  conditional_turn_to_arc: " + std::to_string(resolved.to_arc.size()) + " entries");
	log("  conditional_turn_time_windows: " + std::to_string(n) + " pairs with time window data");

	log("=== Done ===");
	return 0;
}
