#include "../include/graph_utils.h"

#include <routingkit/osm_graph_builder.h>
#include <routingkit/osm_profile.h>
#include <routingkit/vector_io.h>

#include <filesystem>
#include <functional>
#include <iostream>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

namespace {

enum class Profile{
	car,
	motorcycle
};

struct CliArgs{
	std::filesystem::path pbf_path;
	std::filesystem::path output_dir;
	Profile profile = Profile::car;
};

struct GeneratedGraph{
	std::vector<unsigned>first_out;
	std::vector<unsigned>head;
	std::vector<unsigned>way;
	std::vector<unsigned>travel_time;
	std::vector<unsigned>geo_distance;
	std::vector<float>latitude;
	std::vector<float>longitude;
	std::vector<unsigned>forbidden_turn_from_arc;
	std::vector<unsigned>forbidden_turn_to_arc;
};

using WayFilterSignature = bool(*)(uint64_t, const RoutingKit::TagMap&, std::function<void(const std::string&)>);
using WaySpeedSignature = unsigned(*)(uint64_t, const RoutingKit::TagMap&, std::function<void(const std::string&)>);
using DirectionSignature = RoutingKit::OSMWayDirectionCategory(*)(uint64_t, const RoutingKit::TagMap&, std::function<void(const std::string&)>);
using ProfileTurnDecoderSignature = void(*)(uint64_t, const std::vector<RoutingKit::OSMRelationMember>&, const RoutingKit::TagMap&, std::function<void(RoutingKit::OSMTurnRestriction)>, std::function<void(const std::string&)>);

struct ProfileCallbacks{
	const char*name;
	WayFilterSignature is_way_used;
	WaySpeedSignature get_way_speed;
	DirectionSignature get_direction;
	ProfileTurnDecoderSignature decode_turn_restrictions;
};

void print_usage(const char*program){
	std::cerr << "Usage: " << program << " <input.osm.pbf> <output_dir> [--profile car|motorcycle]\n";
}

Profile parse_profile(const std::string&raw_profile){
	if(raw_profile == "car")
		return Profile::car;
	if(raw_profile == "motorcycle")
		return Profile::motorcycle;
	throw std::invalid_argument("Unsupported profile '" + raw_profile + "', expected 'car' or 'motorcycle'");
}

CliArgs parse_args(int argc, char*argv[]){
	if(argc < 3 || argc > 5)
		throw std::invalid_argument("Invalid number of arguments");

	CliArgs args;
	args.pbf_path = argv[1];
	args.output_dir = argv[2];

	if(argc == 3)
		return args;

	const std::string opt = argv[3];
	if(opt == "--profile"){
		if(argc != 5)
			throw std::invalid_argument("Missing value for --profile");
		args.profile = parse_profile(argv[4]);
		return args;
	}

	if(opt.rfind("--profile=", 0) == 0){
		if(argc != 4)
			throw std::invalid_argument("Unexpected trailing arguments");
		args.profile = parse_profile(opt.substr(std::string("--profile=").size()));
		return args;
	}

	throw std::invalid_argument("Unknown option '" + opt + "'");
}

template<typename T>
void save_named_vector(const std::filesystem::path&output_dir, const char*file_name, const std::vector<T>&vec){
	RoutingKit::save_vector((output_dir / file_name).string(), vec);
}

ProfileCallbacks get_profile_callbacks(Profile profile){
	if(profile == Profile::car){
		const WayFilterSignature car_is_way_used = &RoutingKit::is_osm_way_used_by_cars;
		const WaySpeedSignature car_get_way_speed = &RoutingKit::get_osm_way_speed;
		const DirectionSignature car_get_direction = &RoutingKit::get_osm_car_direction_category;
		const ProfileTurnDecoderSignature car_decode_turn_restrictions = &RoutingKit::decode_osm_car_turn_restrictions;
		return {"car", car_is_way_used, car_get_way_speed, car_get_direction, car_decode_turn_restrictions};
	}

	const WayFilterSignature motorcycle_is_way_used = &RoutingKit::is_osm_way_used_by_motorcycles;
	const WaySpeedSignature motorcycle_get_way_speed = &RoutingKit::get_osm_motorcycle_way_speed;
	const DirectionSignature motorcycle_get_direction = &RoutingKit::get_osm_motorcycle_direction_category;
	const ProfileTurnDecoderSignature motorcycle_decode_turn_restrictions = &RoutingKit::decode_osm_motorcycle_turn_restrictions;
	return {"motorcycle", motorcycle_is_way_used, motorcycle_get_way_speed, motorcycle_get_direction, motorcycle_decode_turn_restrictions};
}

GeneratedGraph load_graph(const CliArgs&args){
	// Compile-time API signature checks for intended RoutingKit integration points.
	using MappingLoaderSignature = RoutingKit::OSMRoutingIDMapping(*)(
		const std::string&,
		std::function<bool(uint64_t, const RoutingKit::TagMap&)>,
		std::function<bool(uint64_t, const RoutingKit::TagMap&)>,
		std::function<void(const std::string&)>,
		bool
	);
	using WayCallback = std::function<RoutingKit::OSMWayDirectionCategory(uint64_t, unsigned, const RoutingKit::TagMap&)>;
	using RestrictionDecoder = std::function<void(
		uint64_t,
		const std::vector<RoutingKit::OSMRelationMember>&,
		const RoutingKit::TagMap&,
		std::function<void(RoutingKit::OSMTurnRestriction)>
	)>;
	using GraphLoaderSignature = RoutingKit::OSMRoutingGraph(*)(
		const std::string&,
		const RoutingKit::OSMRoutingIDMapping&,
		WayCallback,
		RestrictionDecoder,
		std::function<void(const std::string&)>,
		bool,
		RoutingKit::OSMRoadGeometry
	);
	const MappingLoaderSignature load_mapping = &RoutingKit::load_osm_id_mapping_from_pbf;
	const GraphLoaderSignature load_routing_graph = &RoutingKit::load_osm_routing_graph_from_pbf;

	const auto log_fn = [](const std::string&msg){
		std::cerr << msg << '\n';
	};

	const ProfileCallbacks profile_callbacks = get_profile_callbacks(args.profile);

	auto mapping = load_mapping(
		args.pbf_path.string(),
		nullptr,
		[&](uint64_t osm_way_id, const RoutingKit::TagMap&way_tags){
			return profile_callbacks.is_way_used(osm_way_id, way_tags, log_fn);
		},
		log_fn,
		true
	);

	std::vector<unsigned>way_speed(mapping.is_routing_way.population_count());

	auto routing_graph = load_routing_graph(
		args.pbf_path.string(),
		mapping,
		[&](uint64_t osm_way_id, unsigned routing_way_id, const RoutingKit::TagMap&way_tags){
			way_speed[routing_way_id] = profile_callbacks.get_way_speed(osm_way_id, way_tags, log_fn);
			return profile_callbacks.get_direction(osm_way_id, way_tags, log_fn);
		},
		[&](
			uint64_t osm_relation_id,
			const std::vector<RoutingKit::OSMRelationMember>&member_list,
			const RoutingKit::TagMap&tags,
			std::function<void(RoutingKit::OSMTurnRestriction)>on_new_restriction
		){
			profile_callbacks.decode_turn_restrictions(osm_relation_id, member_list, tags, std::move(on_new_restriction), log_fn);
		},
		log_fn,
		true,
		RoutingKit::OSMRoadGeometry::none
	);

	mapping = RoutingKit::OSMRoutingIDMapping(); // release memory

	GeneratedGraph out;
	out.travel_time = routing_graph.geo_distance;
	if(routing_graph.way.size() != out.travel_time.size()){
		throw std::runtime_error(
			"RoutingKit returned inconsistent arc metadata for profile '" +
			std::string(profile_callbacks.name) +
			"': way.size() != geo_distance.size()"
		);
	}
	for(std::size_t a=0; a<out.travel_time.size(); ++a){
		const unsigned routing_way_id = routing_graph.way[a];
		if(routing_way_id >= way_speed.size())
			throw std::runtime_error("Encountered out-of-range routing_way_id while computing travel_time");

		const unsigned speed = way_speed[routing_way_id];
		if(speed == 0)
			throw std::runtime_error("Encountered zero way speed while computing travel_time");

		out.travel_time[a] *= 18000;
		out.travel_time[a] /= speed;
		out.travel_time[a] /= 5;
		if(out.travel_time[a] == 0)
			out.travel_time[a] = 1;
	}

	out.first_out = std::move(routing_graph.first_out);
	out.head = std::move(routing_graph.head);
	out.way = std::move(routing_graph.way);
	out.geo_distance = std::move(routing_graph.geo_distance);
	out.latitude = std::move(routing_graph.latitude);
	out.longitude = std::move(routing_graph.longitude);
	out.forbidden_turn_from_arc = std::move(routing_graph.forbidden_turn_from_arc);
	out.forbidden_turn_to_arc = std::move(routing_graph.forbidden_turn_to_arc);
	return out;
}

void save_graph(const std::filesystem::path&output_dir, const GeneratedGraph&graph){
	save_named_vector(output_dir, "first_out", graph.first_out);
	save_named_vector(output_dir, "head", graph.head);
	save_named_vector(output_dir, "way", graph.way);
	save_named_vector(output_dir, "travel_time", graph.travel_time);
	save_named_vector(output_dir, "geo_distance", graph.geo_distance);
	save_named_vector(output_dir, "latitude", graph.latitude);
	save_named_vector(output_dir, "longitude", graph.longitude);
	save_named_vector(output_dir, "forbidden_turn_from_arc", graph.forbidden_turn_from_arc);
	save_named_vector(output_dir, "forbidden_turn_to_arc", graph.forbidden_turn_to_arc);
}

} // namespace

int main(int argc, char*argv[]){
	try{
		const CliArgs args = parse_args(argc, argv);
		if(!std::filesystem::exists(args.pbf_path))
			throw std::runtime_error("Input PBF not found: " + args.pbf_path.string());

		cch_generator::ensure_directory(args.output_dir);
		const GeneratedGraph graph = load_graph(args);
		save_graph(args.output_dir, graph);

		const std::size_t node_count = graph.first_out.empty() ? 0 : graph.first_out.size() - 1;
		cch_generator::print_graph_stats(
			"Generated",
			node_count,
			graph.head.size(),
			graph.forbidden_turn_from_arc.size()
		);
		std::cout << "Output directory: " << args.output_dir.string() << '\n';
		return 0;
	}catch(const std::exception&e){
		print_usage(argv[0]);
		std::cerr << "Error: " << e.what() << '\n';
		return 1;
	}
}
