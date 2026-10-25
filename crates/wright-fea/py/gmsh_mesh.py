"""Solid meshes for wright-fea, written by gmsh's own Abaqus/CalculiX .inp
writer (gmsh src/geo/GModelIO_INP.cpp), so node ordering is gmsh's job.

    python gmsh_mesh.py disk <radius_m> <thickness_m> <size_m> <layers> out.inp
        Flat disk, quads recombined in plane and extruded in `layers`
        (gmsh tutorial t3 / extrude Layers + Recombine), second-order
        serendipity -> 20-node hexahedra (CalculiX C3D20/C3D20R).
    python gmsh_mesh.py plate <lx> <ly> <t> <size_m> out.inp [x,y,r ...]
        Rectangular plate with through holes (OCC box minus cylinders) ->
        10-node tetrahedra. Used where a part's STL comes from a CSG BSP
        (wright-geom) and carries T-junctions that gmsh's STL path rejects
        ("PLC Error: a segment and a facet intersect").
    python gmsh_mesh.py stl <path.stl> <size_m> <scale> out.inp
        Closed STL -> one volume (gmsh tutorial t13: classifySurfaces,
        createGeometry, addSurfaceLoop, addVolume) -> 10-node tetrahedra.
"""
import sys
import gmsh


def disk(r, t, h, layers, out):
    gmsh.model.add("disk")
    occ = gmsh.model.occ
    d = occ.addDisk(0, 0, 0, r, r)
    occ.synchronize()
    gmsh.option.setNumber("Mesh.MeshSizeMax", h)
    gmsh.option.setNumber("Mesh.MeshSizeMin", h * 0.5)
    gmsh.option.setNumber("Mesh.RecombineAll", 1)
    gmsh.option.setNumber("Mesh.Algorithm", 8)  # frontal-Delaunay for quads
    gmsh.model.mesh.setRecombine(2, d)
    occ.extrude([(2, d)], 0, 0, t, numElements=[layers], recombine=True)
    occ.synchronize()
    gmsh.model.addPhysicalGroup(3, [v[1] for v in gmsh.model.getEntities(3)], name="SOLID")


def plate(lx, ly, t, h, holes):
    gmsh.model.add("plate")
    occ = gmsh.model.occ
    b = occ.addBox(0, 0, 0, lx, ly, t)
    cut = [(3, occ.addCylinder(x, y, -t, 0, 0, 3 * t, r)) for (x, y, r) in holes]
    if cut:
        occ.cut([(3, b)], cut)
    occ.synchronize()
    gmsh.option.setNumber("Mesh.MeshSizeMax", h)
    gmsh.option.setNumber("Mesh.MeshSizeMin", h * 0.3)
    gmsh.model.addPhysicalGroup(3, [v[1] for v in gmsh.model.getEntities(3)], name="SOLID")


def stl(path, h, scale, out):
    gmsh.model.add("stl")
    gmsh.merge(path)
    if scale != 1.0:
        tags = [e[1] for e in gmsh.model.getEntities(2)]
        gmsh.model.mesh.affineTransform([scale, 0, 0, 0, 0, scale, 0, 0, 0, 0, scale, 0])
    gmsh.model.mesh.classifySurfaces(40 * 3.14159265 / 180, True, True, 180 * 3.14159265 / 180)
    gmsh.model.mesh.createGeometry()
    surfs = [s[1] for s in gmsh.model.getEntities(2)]
    loop = gmsh.model.geo.addSurfaceLoop(surfs)
    vol = gmsh.model.geo.addVolume([loop])
    gmsh.model.geo.synchronize()
    gmsh.model.addPhysicalGroup(3, [vol], name="SOLID")
    gmsh.option.setNumber("Mesh.MeshSizeMax", h)
    gmsh.option.setNumber("Mesh.MeshSizeMin", h * 0.3)


def main():
    gmsh.initialize(["gmsh", "-v", "2"])
    kind = sys.argv[1]
    if kind == "disk":
        r, t, h, layers, out = float(sys.argv[2]), float(sys.argv[3]), float(sys.argv[4]), int(sys.argv[5]), sys.argv[6]
        disk(r, t, h, layers, out)
        gmsh.option.setNumber("Mesh.SecondOrderIncomplete", 1)
    elif kind == "plate":
        lx, ly, t, h = (float(a) for a in sys.argv[2:6])
        out = sys.argv[6]
        holes = [tuple(float(v) for v in a.split(",")) for a in sys.argv[7:]]
        plate(lx, ly, t, h, holes)
    elif kind == "stl":
        path, h, scale, out = sys.argv[2], float(sys.argv[3]), float(sys.argv[4]), sys.argv[5]
        stl(path, h, scale, out)
    else:
        raise SystemExit(f"unknown kind {kind}")
    gmsh.model.mesh.generate(3)
    gmsh.model.mesh.setOrder(2)
    gmsh.option.setNumber("Mesh.SaveAll", 0)
    gmsh.write(out)
    gmsh.finalize()


if __name__ == "__main__":
    main()
