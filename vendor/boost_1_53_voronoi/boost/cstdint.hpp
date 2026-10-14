// ds-worldgen stand-in for Boost 1.53's boost/cstdint.hpp (original project
// code). The vendored Boost.Polygon voronoi headers only need these four
// typedefs; the real header would pull in all of Boost.Config.
#ifndef DS_BOOST_CSTDINT_HPP
#define DS_BOOST_CSTDINT_HPP
#include <cstdint>
namespace boost {
typedef std::int32_t int32_t;
typedef std::int64_t int64_t;
typedef std::uint32_t uint32_t;
typedef std::uint64_t uint64_t;
}
#endif
