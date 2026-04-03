cmake_minimum_required(VERSION 3.16)
project(CCH_Generator)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# RoutingKit paths (relative to this CMakeLists.txt)
set(ROUTINGKIT_DIR "${CMAKE_SOURCE_DIR}/../RoutingKit")

add_executable(cch_generator src/generate_graph.cpp)
target_include_directories(cch_generator PRIVATE ${ROUTINGKIT_DIR}/include)
target_link_directories(cch_generator PRIVATE ${ROUTINGKIT_DIR}/lib)
target_link_libraries(cch_generator PRIVATE routingkit z pthread m)

find_package(Arrow REQUIRED)
target_link_libraries(cch_generator PRIVATE Arrow::arrow_shared)

add_executable(validate_graph src/validate_graph.cpp)
target_include_directories(validate_graph PRIVATE ${ROUTINGKIT_DIR}/include)
target_link_directories(validate_graph PRIVATE ${ROUTINGKIT_DIR}/lib)
target_link_libraries(validate_graph PRIVATE routingkit z pthread m)
