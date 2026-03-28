#ifndef CCH_GENERATOR_GRAPH_UTILS_H
#define CCH_GENERATOR_GRAPH_UTILS_H

#include <algorithm>
#include <filesystem>
#include <iostream>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

namespace cch_generator {

inline void ensure_directory(const std::filesystem::path&directory){
	std::error_code ec;
	std::filesystem::create_directories(directory, ec);
	if(ec){
		std::ostringstream oss;
		oss << "Failed to create directory '" << directory.string() << "': " << ec.message();
		throw std::runtime_error(oss.str());
	}
}

inline std::vector<unsigned>build_tail(const std::vector<unsigned>&first_out, std::size_t arc_count){
	std::vector<unsigned>tail(arc_count, 0);
	if(first_out.empty())
		return tail;

	for(std::size_t v=0; v+1<first_out.size(); ++v){
		const unsigned begin = std::min<unsigned>(first_out[v], static_cast<unsigned>(arc_count));
		const unsigned end = std::min<unsigned>(first_out[v+1], static_cast<unsigned>(arc_count));
		for(unsigned a=begin; a<end; ++a)
			tail[a] = static_cast<unsigned>(v);
	}
	return tail;
}

inline void print_graph_stats(
	const std::string&label,
	std::size_t node_count,
	std::size_t arc_count,
	std::size_t forbidden_turn_count,
	std::ostream&out = std::cout
){
	const double avg_out_degree = node_count == 0 ? 0.0 : static_cast<double>(arc_count) / static_cast<double>(node_count);
	out << label << " graph statistics:\n";
	out << "  nodes: " << node_count << '\n';
	out << "  arcs: " << arc_count << '\n';
	out << "  avg out-degree: " << avg_out_degree << '\n';
	out << "  forbidden turns: " << forbidden_turn_count << '\n';
}

} // namespace cch_generator

#endif
