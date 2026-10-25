"""Execute wright-geom build programs with FreeCAD's Part kernel (OCCT).

wright_geom::kernel (Rust) records every primitive and boolean the domain
crates call (box, cylinder, ruled loft, revolve, union/subtract/intersect,
translate/rotate) as a JSON tree; this module rebuilds that tree as B-rep
solids and returns OCCT measurements plus a watertight STL per part.

FreeCAD calls used (docstrings read from the installed 1.1.4 bundle; the
implementations are upstream src/Mod/Part/App/AppPartPy.cpp for makeBox /
makeCylinder / makePolygon / makeLoft, src/Mod/Part/App/TopoShapePyImp.cpp
for revolve / fuse / cut / common / translated / rotated / removeSplitter /
distToShape, src/Mod/MeshPart/App/AppMeshPartPy.cpp for meshFromShape and
src/Mod/Mesh/App/MeshPyImp.cpp for hasNonManifolds / isSolid):
  Part.makeBox(l, w, h, pnt)                   -> Box
  Part.makeCylinder(r, h, pnt, dir)            -> Cylinder (exact circle)
  Part.makePolygon(pts) / Part.Face(wire)      -> profiles
  Part.makeLoft(wires, solid=True, ruled=True) -> BRepOffsetAPI_ThruSections
  face.revolve(base, dir, 360)                 -> BRepPrimAPI_MakeRevol
  a.fuse(b) / a.cut(b) / a.common(b)           -> BRepAlgoAPI_Fuse/Cut/Common
  MeshPart.meshFromShape(Shape, LinearDeflection, AngularDeflection, Relative=False)
"""
import math
from pathlib import Path

import FreeCAD as App
import Part
import Mesh
import MeshPart

V = App.Vector


def _v(p):
    return V(float(p['x']), float(p['y']), float(p['z']))


def _frame(axis):
    # Same frame as wright_geom::shapes::frame, so faceted rings line up.
    a = V(axis); a.normalize()
    helper = V(1, 0, 0) if abs(a.x) < 0.9 else V(0, 1, 0)
    u = a.cross(helper); u.normalize()
    return u, a.cross(u)


def _ring(center, u, w, r, n):
    return [center + u * (r * math.cos(2 * math.pi * i / n)) + w * (r * math.sin(2 * math.pi * i / n)) for i in range(n)]


def _loft(rings):
    wires = []
    for ring in rings:
        if isinstance(ring, App.Vector):
            wires.append(Part.Vertex(ring))
        else:
            wires.append(Part.makePolygon(list(ring) + [ring[0]]))
    shape = Part.makeLoft(wires, True, True)
    solid = shape.Solids[0] if shape.Solids else shape
    if solid.Volume < 0:
        solid.reverse()
    return solid


def _revolve_profile(profile, n, faceted):
    pts = [(float(r), float(z)) for r, z in profile]
    if faceted:
        # wright_geom::shapes::revolve: rings at every profile point, ruled
        # between them; a zero radius is an apex, or a flat cap when it
        # shares its neighbour's z.
        sections = []
        for k, (r, z) in enumerate(pts):
            if r <= 1e-12:
                neighbour = pts[k + 1] if k == 0 else pts[k - 1]
                if k in (0, len(pts) - 1) and abs(neighbour[1] - z) > 1e-12:
                    sections.append(V(0, 0, z))
                continue
            sections.append([V(r * math.cos(2 * math.pi * i / n), r * math.sin(2 * math.pi * i / n), z) for i in range(n)])
        return _loft(sections)
    outline = []
    if pts[0][0] > 1e-12:
        outline.append(V(0, 0, pts[0][1]))
    outline += [V(r, 0, z) for r, z in pts]
    if pts[-1][0] > 1e-12:
        outline.append(V(0, 0, pts[-1][1]))
    # drop consecutive duplicates (makePolygon rejects zero-length edges)
    clean = [outline[0]]
    for p in outline[1:]:
        if (p - clean[-1]).Length > 1e-9:
            clean.append(p)
    if (clean[0] - clean[-1]).Length <= 1e-9:
        clean.pop()
    face = Part.Face(Part.makePolygon(clean + [clean[0]]))
    solid = face.revolve(V(0, 0, 0), V(0, 0, 1), 360)
    solid = solid.Solids[0] if solid.Solids else solid
    if solid.Volume < 0:
        solid.reverse()
    return solid


def build(node, faceted=False):
    """One wright-geom program node -> Part.Shape. `faceted` rebuilds
    cylinders and revolves as the same n-gons the in-house mesh used, so
    the translation can be checked for equivalence volume by volume."""
    op = node['op']
    if op == 'box':
        a, b = _v(node['min']), _v(node['max'])
        d = b - a
        return Part.makeBox(d.x, d.y, d.z, a)
    if op == 'cylinder':
        p0, p1, r, n = _v(node['p0']), _v(node['p1']), float(node['r']), int(node['n'])
        axis = p1 - p0
        if faceted:
            u, w = _frame(axis)
            return _loft([_ring(p0, u, w, r, n), _ring(p1, u, w, r, n)])
        return Part.makeCylinder(r, axis.Length, p0, axis)
    if op == 'loft':
        return _loft([[_v(p) for p in ring] for ring in node['rings']])
    if op == 'revolve':
        return _revolve_profile(node['profile'], int(node['n']), faceted)
    if op in ('union', 'subtract', 'intersect'):
        a, b = build(node['a'], faceted), build(node['b'], faceted)
        out = a.fuse(b) if op == 'union' else a.cut(b) if op == 'subtract' else a.common(b)
        try:
            out = out.removeSplitter()
        except Exception:
            pass
        return out
    if op == 'translate':
        return build(node['s'], faceted).translated(_v(node['d']))
    if op == 'rotate':
        return build(node['s'], faceted).rotated(V(0, 0, 0), _v(node['axis']), math.degrees(float(node['angle'])))
    raise ValueError('unknown program op ' + str(op))


def _matrix(m):
    return [[m.A11, m.A12, m.A13], [m.A21, m.A22, m.A23], [m.A31, m.A32, m.A33]]


def _com(shape):
    # volume-weighted over solids (unit density); a single solid is OCCT's own
    total = sum(x.Volume for x in shape.Solids)
    c = sum((x.CenterOfMass * x.Volume for x in shape.Solids), V(0, 0, 0)) * (1.0 / total)
    return [c.x, c.y, c.z]


def _bounds(s):
    b = s.BoundBox
    return dict(min=[b.XMin, b.YMin, b.ZMin], max=[b.XMax, b.YMax, b.ZMax])


def run(items, directory, deflection_mm=0.05, angular_deflection_rad=0.25, faceted_check=False,
        interference=False, exchange=False, stem='assembly'):
    """Build every item, write `<file>.stl` per item, return OCCT evidence.
    A failing item reports its error and the rest still build."""
    out = Path(directory).resolve()
    out.mkdir(parents=True, exist_ok=True)
    results, shapes = [], {}
    for item in items:
        name, file = item['name'], item['file']
        rec = dict(name=name)
        try:
            shape = build(item['program'])
            if shape.isNull() or not shape.Solids:
                raise ValueError('program produced no solid')
            mesh = Mesh.Mesh(MeshPart.meshFromShape(Shape=shape, LinearDeflection=float(deflection_mm),
                                                    AngularDeflection=float(angular_deflection_rad), Relative=False))
            stl = out / (file + '.stl')
            mesh.write(str(stl))
            rec.update(stl=str(stl), volume_mm3=shape.Volume, area_mm2=shape.Area, valid=shape.isValid(),
                       solids=len(shape.Solids), center_of_mass_mm=_com(shape),
                       inertia_mm5=_matrix(shape.Solids[0].MatrixOfInertia) if len(shape.Solids) == 1 else None,
                       bounds=_bounds(shape),
                       mesh=dict(facets=mesh.CountFacets, points=mesh.CountPoints, volume_mm3=mesh.Volume,
                                 non_manifold=mesh.hasNonManifolds(), is_solid=mesh.isSolid(),
                                 misoriented_facets=mesh.countNonUniformOrientedFacets(),
                                 self_intersections=mesh.hasSelfIntersections(),
                                 deflection_mm=deflection_mm, angular_deflection_rad=angular_deflection_rad))
            if faceted_check:
                try:
                    rec['faceted_volume_mm3'] = build(item['program'], faceted=True).Volume
                except Exception as e:
                    rec['faceted_error'] = f'{type(e).__name__}: {e}'
            shapes[name] = shape
        except Exception as e:
            rec['error'] = f'{type(e).__name__}: {e}'
        results.append(rec)
    report = dict(backend='FreeCAD', version='.'.join(App.Version()[:3]), occt=Part.OCC_VERSION, parts=results)
    if interference:
        names = list(shapes)
        pairs = []
        boxes = {n: shapes[n].BoundBox for n in names}
        for i, a in enumerate(names):
            for b in names[i + 1:]:
                if not boxes[a].intersect(boxes[b]):
                    continue
                v = shapes[a].common(shapes[b]).Volume
                if v > 1e-6:
                    pairs.append(dict(a=a, b=b, overlap_mm3=v))
        report['interference'] = pairs
        report['interference_method'] = 'BRepAlgoAPI_Common volume (Shape.common) on bounding-box overlaps'
    if exchange and shapes:
        import exchange as wx
        doc = App.newDocument('WrightKernel')
        try:
            objs = []
            for k, (name, shape) in enumerate(shapes.items()):
                feature = doc.addObject('Part::Feature', f'P{k}')
                feature.Label = name
                feature.Shape = shape
                objs.append(feature)
            doc.recompute()
            step = out / (stem + '.step')
            try:
                report['step'] = dict(path=str(step), schema=wx.export_step(objs, step))
            except Exception as e:
                report['step'] = dict(error=f'{type(e).__name__}: {e}')
            glb = out / (stem + '.glb')
            try:
                report['glb'] = dict(path=str(glb), **wx.export_glb(objs, glb))
            except Exception as e:
                report['glb'] = dict(error=f'{type(e).__name__}: {e}')
            report['labels'] = {f'P{k}': name for k, name in enumerate(shapes)}
        finally:
            App.closeDocument(doc.Name)
    return report
