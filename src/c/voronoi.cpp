// Boost.Polygon 1.53 voronoi construction for ds-worldgen (original project
// code around the vendored Boost headers).
//
// Mirrors the game's BoostMap cell assembly input (fcn.00644670): points are
// inserted first, in Site order (initial indices 0..n-1), then the segments
// (the four world borders), into a voronoi_builder<int32>; the result is a
// voronoi_diagram<double>. The diagram is flattened into index arrays so the
// Rust side can walk it like the game walks the pointer structure.
#include <cstdlib>
#include <cstring>
#include <cstdint>
#include <vector>

#include <boost/polygon/voronoi_builder.hpp>
#include <boost/polygon/voronoi_diagram.hpp>

using boost::polygon::voronoi_builder;
using boost::polygon::voronoi_diagram;

extern "C" {

struct DsVdCell {
  uint32_t source_index;
  uint32_t source_category;  // raw 5-bit category (game: cell->color & 0x1F)
  int32_t incident_edge;     // -1 if none
};

struct DsVdEdge {
  int32_t cell;
  int32_t vertex0;  // -1 for an infinite end
  int32_t twin;
  int32_t next;
  int32_t prev;
  uint32_t is_linear;
  uint32_t is_primary;
};

struct DsVdVertex {
  double x;
  double y;
};

struct DsVd {
  DsVdCell *cells;
  size_t ncells;
  DsVdEdge *edges;
  size_t nedges;
  DsVdVertex *vertices;
  size_t nvertices;
};

int ds_voronoi_build(const int32_t *points, size_t npoints, const int32_t *segments, size_t nsegments,
                     DsVd *out) {
  std::memset(out, 0, sizeof(*out));
  try {
    voronoi_builder<int32_t> vb;
    for (size_t i = 0; i < npoints; ++i) vb.insert_point(points[2 * i], points[2 * i + 1]);
    for (size_t i = 0; i < nsegments; ++i)
      vb.insert_segment(segments[4 * i], segments[4 * i + 1], segments[4 * i + 2], segments[4 * i + 3]);
    voronoi_diagram<double> vd;
    vb.construct(&vd);

    typedef voronoi_diagram<double> vd_t;
    const vd_t::cell_type *cell0 = vd.cells().empty() ? NULL : &vd.cells()[0];
    const vd_t::edge_type *edge0 = vd.edges().empty() ? NULL : &vd.edges()[0];
    const vd_t::vertex_type *vert0 = vd.vertices().empty() ? NULL : &vd.vertices()[0];

    out->ncells = vd.cells().size();
    out->nedges = vd.edges().size();
    out->nvertices = vd.vertices().size();
    out->cells = (DsVdCell *)std::malloc(sizeof(DsVdCell) * (out->ncells ? out->ncells : 1));
    out->edges = (DsVdEdge *)std::malloc(sizeof(DsVdEdge) * (out->nedges ? out->nedges : 1));
    out->vertices = (DsVdVertex *)std::malloc(sizeof(DsVdVertex) * (out->nvertices ? out->nvertices : 1));
    if (!out->cells || !out->edges || !out->vertices) return -2;

    for (size_t i = 0; i < out->ncells; ++i) {
      const vd_t::cell_type &c = vd.cells()[i];
      out->cells[i].source_index = (uint32_t)c.source_index();
      out->cells[i].source_category = (uint32_t)c.source_category();
      out->cells[i].incident_edge = c.incident_edge() ? (int32_t)(c.incident_edge() - edge0) : -1;
    }
    for (size_t i = 0; i < out->nedges; ++i) {
      const vd_t::edge_type &e = vd.edges()[i];
      out->edges[i].cell = (int32_t)(e.cell() - cell0);
      out->edges[i].vertex0 = e.vertex0() ? (int32_t)(e.vertex0() - vert0) : -1;
      out->edges[i].twin = e.twin() ? (int32_t)(e.twin() - edge0) : -1;
      out->edges[i].next = e.next() ? (int32_t)(e.next() - edge0) : -1;
      out->edges[i].prev = e.prev() ? (int32_t)(e.prev() - edge0) : -1;
      out->edges[i].is_linear = e.is_linear() ? 1 : 0;
      out->edges[i].is_primary = e.is_primary() ? 1 : 0;
    }
    for (size_t i = 0; i < out->nvertices; ++i) {
      out->vertices[i].x = vd.vertices()[i].x();
      out->vertices[i].y = vd.vertices()[i].y();
    }
    return 0;
  } catch (...) {
    return -1;
  }
}

void ds_voronoi_free(DsVd *vd) {
  std::free(vd->cells);
  std::free(vd->edges);
  std::free(vd->vertices);
  std::memset(vd, 0, sizeof(*vd));
}

}  // extern "C"
