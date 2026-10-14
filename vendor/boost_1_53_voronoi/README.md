# Boost.Polygon voronoi, Boost 1.53.0 (subset)

The game's native world generator links Boost.Polygon's voronoi builder from
Boost 1.52/1.53 (identified by its n/2n/6n reserve sizes and ULPS 64/128
event comparisons). These are
the unmodified headers from the official boost_1_53_0.tar.gz
(sha256 7c4d1515e0310e7f810cbbc19adb9b2d425f443cc7a00b4599742ee1bdfd4c39),
Boost Software License 1.0 (LICENSE_1_0.txt):

- boost/polygon/voronoi_builder.hpp, voronoi_diagram.hpp, voronoi_geometry_type.hpp
- boost/polygon/detail/voronoi_ctypes.hpp, voronoi_predicates.hpp,
  voronoi_robust_fpt.hpp, voronoi_structures.hpp

`boost/cstdint.hpp` is a four-typedef stand-in written for this project.
