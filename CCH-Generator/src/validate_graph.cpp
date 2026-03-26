#include "../include/graph_utils.h"

#include <routingkit/vector_io.h>

#include <algorithm>
#include <cctype>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <optional>
#include <queue>
#include <sstream>
#include <stdexcept>
#include <string>
#include <unordered_set>
#include <vector>

namespace {

struct PermutationCheckRequest{
	std::filesystem::path perm_file;
	std::optional<std::size_t> expected_size;
};

struct CliArgs{
	std::filesystem::path graph_dir;
	std::optional<std::filesystem::path> turn_expanded_dir;
	std::vector<PermutationCheckRequest>permutation_checks;
};

struct GraphData{
	std::vector<unsigned>first_out;
	std::vector<unsigned>head;
	std::vector<unsigned>travel_time;
	std::vector<unsigned>geo_distance;
	std::vector<float>latitude;
	std::vector<float>longitude;
	std::vector<unsigned>forbidden_turn_from_arc;
	std::vector<unsigned>forbidden_turn_to_arc;

	[[nodiscard]] std::size_t node_count() const{
		return first_out.empty() ? 0 : first_out.size()-1;
	}

	[[nodiscard]] std::size_t arc_count() const{
		return head.size();
	}
};

struct LineGraphData{
	std::vector<unsigned>first_out;
	std::vector<unsigned>head;
	std::vector<unsigned>split_map; // via_way_split_map: split_map[i] = original LG node cloned for split node (base_nodes + i)

	[[nodiscard]] std::size_t node_count() const{
		return first_out.empty() ? 0 : first_out.size()-1;
	}

	/// Resolve a line-graph node ID to the original arc ID it represents.
	/// Base nodes (< original_arc_count) map to themselves.
	/// Split nodes (>= original_arc_count) map through split_map.
	[[nodiscard]] unsigned resolve_to_original_arc(unsigned lg_node, std::size_t original_arc_count) const{
		if(lg_node < original_arc_count)
			return lg_node;
		const std::size_t split_idx = static_cast<std::size_t>(lg_node) - original_arc_count;
		if(split_idx < split_map.size())
			return split_map[split_idx];
		return lg_node; // out of range — caller will detect as invalid
	}
};

struct CheckState{
	bool all_passed = true;
	bool has_warnings = false;
};

void print_usage(const char*program){
	std::cerr << "Usage: " << program
	          << " <graph_dir> [--turn-expanded <line_graph_dir>] [--check-perm <perm_file> [expected_size]]\n";
}

bool is_unsigned_integer(const std::string&value){
	return !value.empty() &&
	       std::all_of(value.begin(), value.end(), [](char c){
		       return std::isdigit(static_cast<unsigned char>(c));
	       });
}

std::size_t parse_size(const std::string&raw, const std::string&context){
	if(!is_unsigned_integer(raw))
		throw std::invalid_argument("Invalid numeric value for " + context + ": '" + raw + "'");
	return static_cast<std::size_t>(std::stoull(raw));
}

CliArgs parse_args(int argc, char*argv[]){
	if(argc < 2)
		throw std::invalid_argument("Missing graph directory");

	CliArgs args;
	args.graph_dir = argv[1];

	for(int i=2; i<argc; ++i){
		const std::string arg = argv[i];
		if(arg == "--turn-expanded"){
			if(i+1 >= argc)
				throw std::invalid_argument("Missing value for --turn-expanded");
			args.turn_expanded_dir = argv[++i];
		}else if(arg == "--check-perm"){
			if(i+1 >= argc)
				throw std::invalid_argument("Missing value for --check-perm");

			PermutationCheckRequest req;
			req.perm_file = argv[++i];
			if(i+1 < argc && is_unsigned_integer(argv[i+1]))
				req.expected_size = parse_size(argv[++i], "--check-perm expected_size");
			args.permutation_checks.push_back(std::move(req));
		}else{
			throw std::invalid_argument("Unknown option '" + arg + "'");
		}
	}
	return args;
}

template<typename T>
std::vector<T>load_required_vector(const std::filesystem::path&dir, const char*name){
	const std::filesystem::path file_path = dir / name;
	if(!std::filesystem::exists(file_path))
		throw std::runtime_error("Missing required file: " + file_path.string());
	return RoutingKit::load_vector<T>(file_path.string());
}

GraphData load_graph(const std::filesystem::path&dir){
	GraphData graph;
	graph.first_out = load_required_vector<unsigned>(dir, "first_out");
	graph.head = load_required_vector<unsigned>(dir, "head");
	graph.travel_time = load_required_vector<unsigned>(dir, "travel_time");
	graph.geo_distance = load_required_vector<unsigned>(dir, "geo_distance");
	graph.latitude = load_required_vector<float>(dir, "latitude");
	graph.longitude = load_required_vector<float>(dir, "longitude");
	graph.forbidden_turn_from_arc = load_required_vector<unsigned>(dir, "forbidden_turn_from_arc");
	graph.forbidden_turn_to_arc = load_required_vector<unsigned>(dir, "forbidden_turn_to_arc");
	return graph;
}

template<typename T>
std::vector<T>load_optional_vector(const std::filesystem::path&dir, const char*name){
	const std::filesystem::path file_path = dir / name;
	if(!std::filesystem::exists(file_path))
		return {};
	return RoutingKit::load_vector<T>(file_path.string());
}

LineGraphData load_line_graph(const std::filesystem::path&dir){
	LineGraphData graph;
	graph.first_out = load_required_vector<unsigned>(dir, "first_out");
	graph.head = load_required_vector<unsigned>(dir, "head");
	graph.split_map = load_optional_vector<unsigned>(dir, "via_way_split_map");
	return graph;
}

void report_check(const std::string&name, bool passed, const std::string&detail, CheckState&state, bool warning = false){
	if(passed){
		std::cout << "[PASS] " << name << " - " << detail << '\n';
		return;
	}

	if(warning){
		std::cout << "[WARN] " << name << " - " << detail << '\n';
		state.has_warnings = true;
		return;
	}

	std::cout << "[FAIL] " << name << " - " << detail << '\n';
	state.all_passed = false;
}

unsigned largest_component_bfs(const std::vector<unsigned>&first_out, const std::vector<unsigned>&head, unsigned node_count){
	if(node_count == 0)
		return 0;

	const auto tail = cch_generator::build_tail(first_out, head.size());
	std::vector<std::vector<unsigned>>adj(node_count);
	for(std::size_t a=0; a<head.size(); ++a){
		const unsigned from = tail[a];
		const unsigned to = head[a];
		if(from >= node_count || to >= node_count)
			continue;
		adj[from].push_back(to);
		adj[to].push_back(from);
	}

	std::vector<char>visited(node_count, 0);
	std::queue<unsigned>q;
	unsigned best = 0;
	for(unsigned start=0; start<node_count; ++start){
		if(visited[start])
			continue;

		unsigned current_size = 0;
		visited[start] = 1;
		q.push(start);
		while(!q.empty()){
			const unsigned u = q.front();
			q.pop();
			++current_size;
			for(unsigned v : adj[u]){
				if(!visited[v]){
					visited[v] = 1;
					q.push(v);
				}
			}
		}
		best = std::max(best, current_size);
	}
	return best;
}

std::uint64_t encode_arc_pair(unsigned from_arc, unsigned to_arc){
	return (static_cast<std::uint64_t>(from_arc) << 32U) | static_cast<std::uint64_t>(to_arc);
}

bool check_permutation_file(
	const std::filesystem::path&file_path,
	const std::optional<std::size_t>&expected_size,
	std::string&detail
){
	if(!std::filesystem::exists(file_path)){
		detail = "Permutation file not found: " + file_path.string();
		return false;
	}

	const auto perm = RoutingKit::load_vector<unsigned>(file_path.string());
	const std::size_t expected_len = expected_size.value_or(perm.size());

	std::ostringstream oss;
	bool passed = true;

	if(expected_size.has_value() && perm.size() != expected_len){
		passed = false;
		oss << "length mismatch (expected " << expected_len << ", got " << perm.size() << "); ";
	}

	std::vector<unsigned char>seen(expected_len, 0);
	std::size_t out_of_range = 0;
	std::size_t duplicates = 0;
	for(unsigned v : perm){
		if(v >= expected_len){
			++out_of_range;
			continue;
		}
		if(seen[v])
			++duplicates;
		else
			seen[v] = 1;
	}

	std::size_t missing = 0;
	for(unsigned char flag : seen){
		if(!flag)
			++missing;
	}

	if(out_of_range > 0 || duplicates > 0 || missing > 0){
		passed = false;
		oss << "out_of_range=" << out_of_range
		    << ", duplicates=" << duplicates
		    << ", missing=" << missing;
	}else{
		oss << "valid permutation with length " << perm.size();
		if(expected_size.has_value())
			oss << " (expected " << expected_len << ")";
	}

	detail = oss.str();
	return passed;
}

bool load_time_window_offsets(
	const std::filesystem::path&file_path,
	std::size_t pair_count,
	std::vector<std::uint32_t>&offsets,
	std::uint64_t&file_size_bytes,
	std::string&detail
){
	std::error_code ec;
	const auto raw_file_size = std::filesystem::file_size(file_path, ec);
	if(ec){
		detail = "failed to read file size: " + file_path.string() + " (" + ec.message() + ")";
		return false;
	}
	file_size_bytes = static_cast<std::uint64_t>(raw_file_size);

	const std::uint64_t offset_count = static_cast<std::uint64_t>(pair_count) + 1ULL;
	const std::uint64_t offset_bytes = offset_count * sizeof(std::uint32_t);
	if(file_size_bytes < offset_bytes){
		std::ostringstream oss;
		oss << "file too small for " << offset_count << " offsets: size=" << file_size_bytes << " bytes";
		detail = oss.str();
		return false;
	}

	offsets.assign(static_cast<std::size_t>(offset_count), 0);
	std::ifstream in(file_path, std::ios::binary);
	if(!in){
		detail = "failed to open file for reading: " + file_path.string();
		return false;
	}

	in.read(reinterpret_cast<char*>(offsets.data()), static_cast<std::streamsize>(offset_bytes));
	if(!in){
		detail = "failed to read offset prefix from: " + file_path.string();
		return false;
	}

	return true;
}

} // namespace

int main(int argc, char*argv[]){
	try{
		const CliArgs args = parse_args(argc, argv);
		if(!std::filesystem::exists(args.graph_dir))
			throw std::runtime_error("Graph directory does not exist: " + args.graph_dir.string());

		const GraphData graph = load_graph(args.graph_dir);
		CheckState state;

		const std::size_t node_count = graph.node_count();
		const std::size_t arc_count = graph.arc_count();
		std::cout << "Validating graph at " << args.graph_dir.string() << '\n';
		cch_generator::print_graph_stats("Loaded", node_count, arc_count, graph.forbidden_turn_from_arc.size());

		bool csr_passed = true;
		{
			std::ostringstream detail;
			if(graph.first_out.empty()){
				csr_passed = false;
				detail << "first_out is empty";
			}else{
				if(graph.first_out.front() != 0){
					csr_passed = false;
					detail << "first_out[0] must be 0; ";
				}
				for(std::size_t i=1; i<graph.first_out.size(); ++i){
					if(graph.first_out[i] < graph.first_out[i-1]){
						csr_passed = false;
						detail << "non-monotonic first_out at index " << i << "; ";
						break;
					}
				}
				if(graph.first_out.back() != arc_count){
					csr_passed = false;
					detail << "first_out[last] must equal arc_count (" << arc_count << ")";
				}
			}
			if(csr_passed)
				detail << "CSR offsets are monotonic and bounded";
			report_check("CSR structure", csr_passed, detail.str(), state);
		}

		bool head_bounds_passed = true;
		{
			std::size_t invalid_head_count = 0;
			for(unsigned h : graph.head){
				if(h >= node_count)
					++invalid_head_count;
			}
			head_bounds_passed = invalid_head_count == 0;
			std::ostringstream detail;
			if(head_bounds_passed)
				detail << "all head indices within [0, " << node_count << ')';
			else
				detail << invalid_head_count << " arcs point outside node range";
			report_check("Head bounds", head_bounds_passed, detail.str(), state);
		}

		const auto tail = cch_generator::build_tail(graph.first_out, arc_count);
		{
			if(!csr_passed || !head_bounds_passed){
				report_check(
					"No self-loops",
					false,
					"skipped because CSR structure or head bounds failed",
					state,
					true
				);
			}else{
				std::size_t self_loop_count = 0;
				for(std::size_t a=0; a<arc_count; ++a){
					if(tail[a] < node_count && graph.head[a] < node_count && tail[a] == graph.head[a])
						++self_loop_count;
				}
				const bool pass = self_loop_count == 0;
				std::ostringstream detail;
				if(pass)
					detail << "no self-loops detected";
				else
					detail << self_loop_count << " self-loops detected";
				report_check("No self-loops", pass, detail.str(), state);
			}
		}

		bool vector_lengths_passed = true;
		{
			std::ostringstream detail;
			if(graph.head.size() != graph.travel_time.size()){
				vector_lengths_passed = false;
				detail << "head/travel_time mismatch; ";
			}
			if(graph.head.size() != graph.geo_distance.size()){
				vector_lengths_passed = false;
				detail << "head/geo_distance mismatch; ";
			}
			if(graph.latitude.size() != node_count || graph.longitude.size() != node_count){
				vector_lengths_passed = false;
				detail << "coordinate length mismatch";
			}
			if(vector_lengths_passed)
				detail << "all core vectors have consistent lengths";
			report_check("Vector length consistency", vector_lengths_passed, detail.str(), state);
		}

		{
			std::size_t invalid_coord_count = 0;
			const std::size_t coord_count = std::min(graph.latitude.size(), graph.longitude.size());
			for(std::size_t i=0; i<coord_count; ++i){
				const float lat = graph.latitude[i];
				const float lon = graph.longitude[i];
				if(lat < 0.0f || lat > 30.0f || lon < 100.0f || lon > 115.0f)
					++invalid_coord_count;
			}
			const bool pass = invalid_coord_count == 0;
			std::ostringstream detail;
			if(pass)
				detail << "all coordinates in Vietnam bounds";
			else
				detail << invalid_coord_count << " coordinates outside [lat 0..30, lon 100..115]";
			report_check("Coordinate sanity", pass, detail.str(), state);
		}

		{
			std::size_t zero_travel_time = 0;
			std::size_t too_large_travel_time = 0;
			std::vector<std::size_t>zero_samples;
			std::vector<std::size_t>too_large_samples;
			constexpr std::size_t max_samples = 5;
			constexpr unsigned max_reasonable = 86400000U;
			for(std::size_t a=0; a<graph.travel_time.size(); ++a){
				const unsigned tt = graph.travel_time[a];
				if(tt == 0){
					++zero_travel_time;
					if(zero_samples.size() < max_samples)
						zero_samples.push_back(a);
				}
				if(tt > max_reasonable){
					++too_large_travel_time;
					if(too_large_samples.size() < max_samples)
						too_large_samples.push_back(a);
				}
			}

			const auto append_sample_list = [](std::ostringstream&oss, const std::vector<std::size_t>&samples){
				oss << " (sample_arc_ids=";
				for(std::size_t i=0; i<samples.size(); ++i){
					if(i > 0)
						oss << ',';
					oss << samples[i];
				}
				oss << ")";
			};

			std::ostringstream detail;
			if(too_large_travel_time > 0){
				detail << too_large_travel_time << " arcs exceed 24h travel time";
				if(!too_large_samples.empty())
					append_sample_list(detail, too_large_samples);
				if(zero_travel_time > 0){
					detail << "; zero_travel_time=" << zero_travel_time;
					if(!zero_samples.empty())
						append_sample_list(detail, zero_samples);
				}
				report_check("Travel time sanity", false, detail.str(), state, true);
			}else if(zero_travel_time > 0){
				detail << zero_travel_time << " zero travel-time arcs (possible ferries or data artifacts)";
				if(!zero_samples.empty())
					append_sample_list(detail, zero_samples);
				report_check("Travel time sanity", false, detail.str(), state, true);
			}else{
				detail << "all travel times are > 0 and <= 24h";
				report_check("Travel time sanity", true, detail.str(), state);
			}
		}

		{
			if(!vector_lengths_passed || !csr_passed || !head_bounds_passed){
				report_check(
					"Travel time anomaly tracking",
					false,
					"skipped because vector length consistency, CSR structure, or head bounds failed",
					state,
					true
				);
			}else{
				constexpr unsigned min_distance_for_speed_check_m = 100U;
				constexpr unsigned max_reasonable_speed_kmh = 180U;
				constexpr unsigned min_reasonable_speed_kmh = 1U;
				constexpr std::size_t max_samples = 5;

				std::size_t fast_arc_count = 0;
				std::size_t slow_arc_count = 0;
				std::vector<std::size_t>fast_samples;
				std::vector<std::size_t>slow_samples;
				for(std::size_t a=0; a<arc_count; ++a){
					const unsigned tt = graph.travel_time[a];
					const unsigned dist = graph.geo_distance[a];
					if(tt == 0 || dist < min_distance_for_speed_check_m)
						continue;

					const double speed_kmh = static_cast<double>(dist) * 3600.0 / static_cast<double>(tt);
					if(speed_kmh > static_cast<double>(max_reasonable_speed_kmh)){
						++fast_arc_count;
						if(fast_samples.size() < max_samples)
							fast_samples.push_back(a);
					}
					if(speed_kmh < static_cast<double>(min_reasonable_speed_kmh)){
						++slow_arc_count;
						if(slow_samples.size() < max_samples)
							slow_samples.push_back(a);
					}
				}

				const auto append_samples = [&](std::ostringstream&oss, const char*label, const std::vector<std::size_t>&samples){
					oss << "; " << label << "_samples=";
					for(std::size_t i=0; i<samples.size(); ++i){
						if(i > 0)
							oss << ", ";
						const std::size_t a = samples[i];
						const double speed_kmh = static_cast<double>(graph.geo_distance[a]) * 3600.0 / static_cast<double>(graph.travel_time[a]);
						oss << 'a' << a << '('
						    << tail[a] << "->" << graph.head[a]
						    << ", tt=" << graph.travel_time[a] << "ms"
						    << ", dist=" << graph.geo_distance[a] << "m"
						    << ", speed=" << std::fixed << std::setprecision(1) << speed_kmh << "km/h" << std::defaultfloat
						    << ')';
					}
				};

				const bool has_anomaly = fast_arc_count > 0 || slow_arc_count > 0;
				std::ostringstream detail;
				detail << "distance>=" << min_distance_for_speed_check_m
				       << "m arcs checked for speed outliers; fast(>" << max_reasonable_speed_kmh
				       << "km/h)=" << fast_arc_count << ", slow(<" << min_reasonable_speed_kmh
				       << "km/h)=" << slow_arc_count;
				if(!fast_samples.empty())
					append_samples(detail, "fast", fast_samples);
				if(!slow_samples.empty())
					append_samples(detail, "slow", slow_samples);
				report_check("Travel time anomaly tracking", !has_anomaly, detail.str(), state, has_anomaly);
			}
		}

		{
			bool pass = graph.forbidden_turn_from_arc.size() == graph.forbidden_turn_to_arc.size();
			std::size_t inversion_index = 0;
			for(std::size_t i=1; pass && i<graph.forbidden_turn_from_arc.size(); ++i){
				const unsigned prev_from = graph.forbidden_turn_from_arc[i-1];
				const unsigned prev_to = graph.forbidden_turn_to_arc[i-1];
				const unsigned curr_from = graph.forbidden_turn_from_arc[i];
				const unsigned curr_to = graph.forbidden_turn_to_arc[i];
				if(curr_from < prev_from || (curr_from == prev_from && curr_to < prev_to)){
					pass = false;
					inversion_index = i;
				}
			}

			std::ostringstream detail;
			if(pass){
				detail << "forbidden turns are sorted lexicographically by (from_arc, to_arc)";
			}else if(graph.forbidden_turn_from_arc.size() != graph.forbidden_turn_to_arc.size()){
				detail << "forbidden turn vectors have different sizes";
			}else{
				detail << "lexicographic inversion at index " << inversion_index
				       << ": (" << graph.forbidden_turn_from_arc[inversion_index-1] << ", " << graph.forbidden_turn_to_arc[inversion_index-1] << ") then ("
				       << graph.forbidden_turn_from_arc[inversion_index] << ", " << graph.forbidden_turn_to_arc[inversion_index] << ")";
			}
			report_check("Turn restriction sorting", pass, detail.str(), state);
		}

		{
			bool pass = graph.forbidden_turn_from_arc.size() == graph.forbidden_turn_to_arc.size();
			std::size_t out_of_bounds_turns = 0;
			const std::size_t turn_count = std::min(graph.forbidden_turn_from_arc.size(), graph.forbidden_turn_to_arc.size());
			for(std::size_t i=0; i<turn_count; ++i){
				if(graph.forbidden_turn_from_arc[i] >= arc_count || graph.forbidden_turn_to_arc[i] >= arc_count)
					++out_of_bounds_turns;
			}
			if(out_of_bounds_turns > 0)
				pass = false;

			std::ostringstream detail;
			if(pass)
				detail << "all forbidden turns reference valid arc IDs";
			else if(graph.forbidden_turn_from_arc.size() != graph.forbidden_turn_to_arc.size())
				detail << "turn vectors differ in length";
			else
				detail << out_of_bounds_turns << " forbidden turns reference out-of-range arc IDs";
			report_check("Turn restriction arc bounds", pass, detail.str(), state);
		}

		{
			std::filesystem::path conditional_dir = args.graph_dir / "conditional_turns";
			if(!std::filesystem::exists(conditional_dir) && args.graph_dir.filename() == "graph"){
				const std::filesystem::path sibling_conditional_dir = args.graph_dir.parent_path() / "conditional_turns";
				if(std::filesystem::exists(sibling_conditional_dir))
					conditional_dir = sibling_conditional_dir;
			}
			const std::filesystem::path conditional_from_file = conditional_dir / "conditional_turn_from_arc";
			const std::filesystem::path conditional_to_file = conditional_dir / "conditional_turn_to_arc";
			const std::filesystem::path conditional_windows_file = conditional_dir / "conditional_turn_time_windows";

			const bool has_conditional_from = std::filesystem::exists(conditional_from_file);
			const bool has_conditional_to = std::filesystem::exists(conditional_to_file);
			const bool has_conditional_windows = std::filesystem::exists(conditional_windows_file);

			if(has_conditional_from || has_conditional_to || has_conditional_windows){
				const bool all_conditional_files = has_conditional_from && has_conditional_to && has_conditional_windows;
				{
					std::ostringstream detail;
					detail << "from=" << (has_conditional_from ? "yes" : "no")
					       << ", to=" << (has_conditional_to ? "yes" : "no")
					       << ", windows=" << (has_conditional_windows ? "yes" : "no");
					report_check("Conditional turn files present", all_conditional_files, detail.str(), state);
				}

				if(all_conditional_files){
					const auto conditional_from_arc = RoutingKit::load_vector<unsigned>(conditional_from_file.string());
					const auto conditional_to_arc = RoutingKit::load_vector<unsigned>(conditional_to_file.string());

					const bool conditional_pair_size_ok = conditional_from_arc.size() == conditional_to_arc.size();
					{
						std::ostringstream detail;
						detail << "from_size=" << conditional_from_arc.size()
						       << ", to_size=" << conditional_to_arc.size();
						report_check("Conditional turn vector consistency", conditional_pair_size_ok, detail.str(), state);
					}

					{
						std::size_t out_of_bounds_turns = 0;
						const std::size_t conditional_pair_count = std::min(conditional_from_arc.size(), conditional_to_arc.size());
						for(std::size_t i=0; i<conditional_pair_count; ++i){
							if(conditional_from_arc[i] >= arc_count || conditional_to_arc[i] >= arc_count)
								++out_of_bounds_turns;
						}

						bool pass = conditional_pair_size_ok && out_of_bounds_turns == 0;
						std::ostringstream detail;
						if(!conditional_pair_size_ok){
							detail << "conditional pair vectors have different sizes";
						}else if(out_of_bounds_turns == 0){
							detail << "all conditional turns reference valid arc IDs";
						}else{
							detail << out_of_bounds_turns << " conditional turns reference out-of-range arc IDs";
						}
						report_check("Conditional turn arc bounds", pass, detail.str(), state);
					}

					{
						bool pass = conditional_pair_size_ok;
						std::size_t inversion_index = 0;
						for(std::size_t i=1; pass && i<conditional_from_arc.size(); ++i){
							const unsigned prev_from = conditional_from_arc[i-1];
							const unsigned prev_to = conditional_to_arc[i-1];
							const unsigned curr_from = conditional_from_arc[i];
							const unsigned curr_to = conditional_to_arc[i];
							if(curr_from < prev_from || (curr_from == prev_from && curr_to < prev_to)){
								pass = false;
								inversion_index = i;
							}
						}

						std::ostringstream detail;
						if(!conditional_pair_size_ok){
							detail << "conditional pair vectors have different sizes";
						}else if(pass){
							detail << "conditional turn pairs are sorted lexicographically by (from_arc, to_arc)";
						}else{
							detail << "lexicographic inversion at index " << inversion_index
							       << ": (" << conditional_from_arc[inversion_index-1] << ", " << conditional_to_arc[inversion_index-1] << ") then ("
							       << conditional_from_arc[inversion_index] << ", " << conditional_to_arc[inversion_index] << ")";
						}
						report_check(
							"Conditional turn sorting",
							pass,
							detail.str(),
							state
						);
					}

					{
						std::vector<std::uint32_t>offsets;
						std::uint64_t window_file_size = 0;
						std::string load_detail;
						const bool offset_prefix_loaded = load_time_window_offsets(
							conditional_windows_file,
							conditional_from_arc.size(),
							offsets,
							window_file_size,
							load_detail
						);

						bool pass = offset_prefix_loaded;
						std::ostringstream detail;
						if(!offset_prefix_loaded){
							detail << load_detail;
						}else{
							bool monotonic = true;
							for(std::size_t i=1; i<offsets.size(); ++i){
								if(offsets[i] < offsets[i-1]){
									monotonic = false;
									break;
								}
							}

							const std::uint64_t offset_bytes = static_cast<std::uint64_t>(offsets.size()) * sizeof(std::uint32_t);
							const std::uint64_t packed_entry_count = offsets.back();
							const std::uint64_t expected_total_size = offset_bytes + packed_entry_count * 5ULL;
							const bool size_matches = expected_total_size == window_file_size;
							pass = monotonic && size_matches;

							detail << "offsets=" << offsets.size()
							       << ", packed_entries=" << packed_entry_count
							       << ", expected_bytes=" << expected_total_size
							       << ", file_bytes=" << window_file_size;
							if(!monotonic)
								detail << ", offsets_not_monotonic";
							if(!size_matches)
								detail << ", packed_size_mismatch";
						}
						report_check("Time window file integrity", pass, detail.str(), state);
					}

					{
						const bool forbidden_pair_size_ok = graph.forbidden_turn_from_arc.size() == graph.forbidden_turn_to_arc.size();
						if(!forbidden_pair_size_ok || !conditional_pair_size_ok){
							report_check(
								"No overlap with forbidden turns",
								false,
								"skipped because forbidden/conditional turn vectors are inconsistent",
								state,
								true
							);
						}else{
							std::unordered_set<std::uint64_t>forbidden_turns;
							forbidden_turns.reserve(graph.forbidden_turn_from_arc.size());
							for(std::size_t i=0; i<graph.forbidden_turn_from_arc.size(); ++i){
								forbidden_turns.insert(encode_arc_pair(graph.forbidden_turn_from_arc[i], graph.forbidden_turn_to_arc[i]));
							}

							std::size_t overlap_count = 0;
							for(std::size_t i=0; i<conditional_from_arc.size(); ++i){
								if(forbidden_turns.find(encode_arc_pair(conditional_from_arc[i], conditional_to_arc[i])) != forbidden_turns.end())
									++overlap_count;
							}

							const bool pass = overlap_count == 0;
							std::ostringstream detail;
							if(pass)
								detail << "no overlapping (from_arc,to_arc) pairs";
							else
								detail << overlap_count << " conditional turns overlap forbidden turns";
							report_check("No overlap with forbidden turns", pass, detail.str(), state);
						}
					}
				}
			}else{
				std::cout << "[INFO] Conditional turn files not detected; skipping conditional checks\n";
			}
		}

		// Via-way chain file validation
		{
			const std::filesystem::path offsets_file = args.graph_dir / "via_way_chain_offsets";
			const std::filesystem::path arcs_file = args.graph_dir / "via_way_chain_arcs";
			const std::filesystem::path mandatory_file = args.graph_dir / "via_way_chain_mandatory";

			const bool has_offsets = std::filesystem::exists(offsets_file);
			const bool has_arcs = std::filesystem::exists(arcs_file);
			const bool has_mandatory = std::filesystem::exists(mandatory_file);

			if(has_offsets || has_arcs || has_mandatory){
				const bool all_present = has_offsets && has_arcs && has_mandatory;
				{
					std::ostringstream detail;
					detail << "offsets=" << (has_offsets ? "yes" : "no")
					       << ", arcs=" << (has_arcs ? "yes" : "no")
					       << ", mandatory=" << (has_mandatory ? "yes" : "no");
					report_check("Via-way chain files present", all_present, detail.str(), state);
				}

				if(all_present){
					const auto chain_offsets = RoutingKit::load_vector<unsigned>(offsets_file.string());
					const auto chain_arcs = RoutingKit::load_vector<unsigned>(arcs_file.string());
					const auto chain_mandatory = RoutingKit::load_vector<uint8_t>(mandatory_file.string());

					// Offsets must be non-empty (at least sentinel 0)
					const bool offsets_nonempty = !chain_offsets.empty();
					const std::size_t chain_count = offsets_nonempty ? chain_offsets.size() - 1 : 0;

					// CSR structure: offsets monotonic, first == 0, last == arcs.size()
					bool offsets_valid = offsets_nonempty;
					{
						std::ostringstream detail;
						if(!offsets_nonempty){
							offsets_valid = false;
							detail << "offsets vector is empty";
						}else{
							if(chain_offsets.front() != 0){
								offsets_valid = false;
								detail << "offsets[0] != 0; ";
							}
							if(chain_offsets.back() != chain_arcs.size()){
								offsets_valid = false;
								detail << "offsets[last] (" << chain_offsets.back() << ") != arcs length (" << chain_arcs.size() << "); ";
							}
							for(std::size_t i=1; i<chain_offsets.size(); ++i){
								if(chain_offsets[i] < chain_offsets[i-1]){
									offsets_valid = false;
									detail << "non-monotonic at index " << i << "; ";
									break;
								}
							}
							if(offsets_valid)
								detail << chain_count << " chains, " << chain_arcs.size() << " total arcs, CSR structure valid";
						}
						report_check("Via-way chain CSR structure", offsets_valid, detail.str(), state);
					}

					// Mandatory vector length must match chain count
					{
						const bool mandatory_size_ok = chain_mandatory.size() == chain_count;
						std::ostringstream detail;
						detail << "mandatory_size=" << chain_mandatory.size() << ", chain_count=" << chain_count;
						report_check("Via-way chain mandatory vector consistency", mandatory_size_ok, detail.str(), state);
					}

					// All arcs must reference valid original arc IDs
					{
						std::size_t out_of_bounds = 0;
						for(unsigned a : chain_arcs){
							if(a >= arc_count)
								++out_of_bounds;
						}
						const bool pass = out_of_bounds == 0;
						std::ostringstream detail;
						if(pass)
							detail << "all " << chain_arcs.size() << " chain arc IDs within [0, " << arc_count << ')';
						else
							detail << out_of_bounds << " chain arc IDs out of range (>= " << arc_count << ')';
						report_check("Via-way chain arc bounds", pass, detail.str(), state);
					}

					// Each chain must have length >= 3 (from_arc, at least one via_arc, to_arc)
					if(offsets_valid){
						std::size_t short_chain_count = 0;
						for(std::size_t i=0; i<chain_count; ++i){
							const std::size_t chain_len = chain_offsets[i+1] - chain_offsets[i];
							if(chain_len < 3)
								++short_chain_count;
						}
						const bool pass = short_chain_count == 0;
						std::ostringstream detail;
						if(pass)
							detail << "all " << chain_count << " chains have length >= 3";
						else
							detail << short_chain_count << " chains have length < 3 (need at least from_arc + via_arc + to_arc)";
						report_check("Via-way chain minimum length", pass, detail.str(), state);
					}
				}
			}else{
				std::cout << "[INFO] Via-way chain files not detected; skipping via-way chain checks\n";
			}
		}

		std::size_t isolated_nodes = 0;
		{
			if(!csr_passed || !head_bounds_passed){
				report_check(
					"No isolated nodes",
					false,
					"skipped because CSR structure or head bounds failed",
					state,
					true
				);
			}else{
				std::vector<unsigned>degree(node_count, 0);
				for(std::size_t a=0; a<arc_count; ++a){
					if(tail[a] < node_count)
						++degree[tail[a]];
					if(graph.head[a] < node_count)
						++degree[graph.head[a]];
				}
				for(unsigned deg : degree){
					if(deg == 0)
						++isolated_nodes;
				}

				const double isolated_ratio = node_count == 0 ? 0.0 : static_cast<double>(isolated_nodes) / static_cast<double>(node_count);
				std::ostringstream detail;
				detail << isolated_nodes << " isolated nodes (" << (isolated_ratio * 100.0) << "%)";
				const bool warning = isolated_ratio > 0.01;
				report_check("No isolated nodes", !warning, detail.str(), state, warning);
			}
		}

		unsigned largest_component = 0;
		{
			if(!csr_passed || !head_bounds_passed){
				report_check(
					"Graph connectivity",
					false,
					"skipped because CSR structure or head bounds failed",
					state,
					true
				);
			}else{
				largest_component = largest_component_bfs(graph.first_out, graph.head, static_cast<unsigned>(node_count));
				const double ratio = node_count == 0 ? 1.0 : static_cast<double>(largest_component) / static_cast<double>(node_count);
				const bool warning = ratio < 0.90;
				std::ostringstream detail;
				detail << "largest component size " << largest_component << " (" << (ratio * 100.0) << "%)";
				report_check("Graph connectivity", !warning, detail.str(), state, warning);
			}
		}

		std::cout << "Connectivity summary: isolated_nodes=" << isolated_nodes
		          << ", largest_component=" << largest_component << '\n';

		for(const auto&perm_check : args.permutation_checks){
			std::string detail;
			const bool pass = check_permutation_file(perm_check.perm_file, perm_check.expected_size, detail);
			std::ostringstream title;
			title << "Permutation validity (" << perm_check.perm_file.string() << ')';
			report_check(title.str(), pass, detail, state);
		}

		if(args.turn_expanded_dir.has_value()){
			if(!std::filesystem::exists(*args.turn_expanded_dir))
				throw std::runtime_error("Line graph directory does not exist: " + args.turn_expanded_dir->string());
			const LineGraphData line_graph = load_line_graph(*args.turn_expanded_dir);
			const std::size_t split_count = line_graph.split_map.size();

			// Line graph node count: base nodes (= original arcs) + split nodes
			{
				const std::size_t expected_node_count = arc_count + split_count;
				const bool line_node_count_ok = line_graph.node_count() == expected_node_count;
				std::ostringstream detail;
				if(line_node_count_ok){
					detail << "line graph node_count (" << line_graph.node_count() << ") = arc_count (" << arc_count << ") + split_nodes (" << split_count << ')';
				}else{
					detail << "line graph node_count (" << line_graph.node_count() << ") != expected (" << expected_node_count << " = arc_count " << arc_count << " + split_nodes " << split_count << ')';
				}
				report_check("Line graph node count", line_node_count_ok, detail.str(), state);
			}

			// Split map validity: each entry must reference a valid base LG node (< arc_count)
			{
				std::size_t out_of_range_count = 0;
				for(unsigned v : line_graph.split_map){
					if(v >= arc_count)
						++out_of_range_count;
				}
				const bool pass = out_of_range_count == 0;
				std::ostringstream detail;
				if(split_count == 0){
					detail << "no split nodes (via_way_split_map empty or absent)";
				}else if(pass){
					detail << "all " << split_count << " split_map entries reference valid base LG nodes [0, " << arc_count << ')';
				}else{
					detail << out_of_range_count << " split_map entries reference out-of-range base LG node IDs (>= " << arc_count << ')';
				}
				report_check("Split map validity", pass, detail.str(), state);
			}

			if(!csr_passed || !head_bounds_passed){
				report_check(
					"No forbidden turns in line graph",
					false,
					"skipped because base graph CSR structure or head bounds failed",
					state,
					true
				);
				report_check(
					"Line graph transition consistency",
					false,
					"skipped because base graph CSR structure or head bounds failed",
					state,
					true
				);
			}else{
				const auto line_tail = cch_generator::build_tail(line_graph.first_out, line_graph.head.size());
				const auto original_tail = cch_generator::build_tail(graph.first_out, arc_count);

				std::unordered_set<std::uint64_t>forbidden_turns;
				forbidden_turns.reserve(graph.forbidden_turn_from_arc.size());
				const std::size_t forbidden_pair_count = std::min(graph.forbidden_turn_from_arc.size(), graph.forbidden_turn_to_arc.size());
				for(std::size_t i=0; i<forbidden_pair_count; ++i){
					forbidden_turns.insert(encode_arc_pair(graph.forbidden_turn_from_arc[i], graph.forbidden_turn_to_arc[i]));
				}

				// Resolve LG node IDs to original arc IDs before checking transitions.
				// Split nodes (>= arc_count) map through split_map to their cloned base node.
				std::size_t invalid_transition_count = 0;
				std::size_t forbidden_transition_count = 0;
				std::size_t disconnected_transition_count = 0;
				for(std::size_t le=0; le<line_graph.head.size(); ++le){
					const unsigned raw_from = line_tail[le];
					const unsigned raw_to = line_graph.head[le];

					const unsigned from_arc = line_graph.resolve_to_original_arc(raw_from, arc_count);
					const unsigned to_arc = line_graph.resolve_to_original_arc(raw_to, arc_count);

					if(from_arc >= arc_count || to_arc >= arc_count){
						++invalid_transition_count;
						continue;
					}
					if(forbidden_turns.find(encode_arc_pair(from_arc, to_arc)) != forbidden_turns.end())
						++forbidden_transition_count;

					if(graph.head[from_arc] != original_tail[to_arc])
						++disconnected_transition_count;
				}

				{
					std::ostringstream detail;
					if(forbidden_transition_count == 0)
						detail << "no forbidden-turn transitions found in line graph";
					else
						detail << forbidden_transition_count << " forbidden-turn transitions detected";
					report_check("No forbidden turns in line graph", forbidden_transition_count == 0, detail.str(), state);
				}

				{
					std::ostringstream detail;
					if(invalid_transition_count == 0 && disconnected_transition_count == 0){
						detail << "all " << line_graph.head.size() << " transitions map to valid consecutive original arcs (resolved through " << split_count << " split nodes)";
						report_check("Line graph transition consistency", true, detail.str(), state);
					}else{
						detail << "invalid_transition_count=" << invalid_transition_count
						       << ", disconnected_transition_count=" << disconnected_transition_count;
						report_check("Line graph transition consistency", false, detail.str(), state);
					}
				}
			}

			const std::filesystem::path cch_exp_perm = (*args.turn_expanded_dir) / "cch_exp_perm";
			if(std::filesystem::exists(cch_exp_perm)){
				std::string detail;
				const bool pass = check_permutation_file(cch_exp_perm, line_graph.node_count(), detail);
				report_check("Turn-expanded permutation validity", pass, detail, state);
			}else{
				report_check(
					"Turn-expanded permutation validity",
					false,
					"cch_exp_perm not found (skipping)",
					state,
					true
				);
			}
		}

		if(state.all_passed){
			std::cout << "Validation result: PASS";
			if(state.has_warnings)
				std::cout << " (with warnings)";
			std::cout << '\n';
			return 0;
		}

		std::cout << "Validation result: FAIL\n";
		return 1;
	}catch(const std::exception&e){
		print_usage(argv[0]);
		std::cerr << "Error: " << e.what() << '\n';
		return 1;
	}
}
