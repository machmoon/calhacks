"""Open CASCADE work behind the worker: build one part, place and check an
assembly, write STEP AP242 / GLB / STL.

Kernel choice and evidence: docs/ARCHITECTURE.md "CAD". Mass properties are
OCC's `BRepGProp.VolumeProperties` (exact on the B-rep, not a mesh);
validity is `BRepCheck_Analyzer`; interference is `BRepAlgoAPI_Common`
volume and clearance `BRepExtrema_DistShapeShape`, over all pairs whose
boxes come within the clearance window (the all-pairs rule of
docs/ARCHITECTURE.md, with a bounding-box broad phase).
"""
from __future__ import annotations

import json
import math
import time
from pathlib import Path

from build123d import Color, Compound, Location, Part, Shape, Unit, export_gltf, export_stl, import_brep
from build123d.exporters3d import _create_xde
from OCP.APIHeaderSection import APIHeaderSection_MakeHeader
from OCP.BRepAlgoAPI import BRepAlgoAPI_Common
from OCP.BRepCheck import BRepCheck_Analyzer
from OCP.BRepExtrema import BRepExtrema_DistShapeShape
from OCP.BRepGProp import BRepGProp
from OCP.BRepTools import BRepTools
from OCP.GProp import GProp_GProps
from OCP.gp import gp_Pnt, gp_Trsf
from OCP.IFSelect import IFSelect_ReturnStatus
from OCP.Interface import Interface_Static
from OCP.Message import Message, Message_Gravity
from OCP.STEPCAFControl import STEPCAFControl_Controller, STEPCAFControl_Writer
from OCP.STEPControl import STEPControl_Controller, STEPControl_StepModelType
from OCP.TCollection import TCollection_HAsciiString
from OCP.XSControl import XSControl_WorkSession

from .validation import finite_number, validate_instances, pair_result
from . import library
from .materials import MATERIALS

AP242 = "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }"
STL_TOL_MM = 0.02  # chordal deviation of printed-part STLs (finer than a 0.4 mm nozzle resolves)
STL_ANGLE_RAD = 0.1
GLB_TOL_MM = 0.1


# --------------------------------------------------------------------------------------
# STEP AP242 with names and colours. build123d's `export_step`
# (build123d/exporters3d.py) writes AP214 (OCCT's default `write.step.schema`)
# and builds its header with MakeHeader(0), which ignores the schema static, so
# this is that function with the schema set before the writer exists and the
# header's schema identifier set to AP242.
def export_step_ap242(shape: Shape, path: str | Path, timestamp: str = "2026-01-01T00:00:00") -> None:
    STEPControl_Controller.Init_s()
    STEPCAFControl_Controller.Init_s()
    Interface_Static.SetCVal_s("write.step.schema", "AP242DIS")
    doc = _create_xde(shape, Unit.MM, auto_naming=True)
    for printer in Message.DefaultMessenger_s().Printers():
        printer.SetTraceLevel(Message_Gravity.Message_Fail)
    writer = STEPCAFControl_Writer(XSControl_WorkSession(), False)
    writer.SetColorMode(True)
    writer.SetLayerMode(True)
    writer.SetNameMode(True)
    model = writer.Writer().Model()
    header = APIHeaderSection_MakeHeader(model)
    if not header.IsDone():
        header = APIHeaderSection_MakeHeader(0)
        header.Apply(model)
    if shape.label:
        header.SetName(TCollection_HAsciiString(shape.label))
    header.SetTimeStamp(TCollection_HAsciiString(timestamp))  # fixed: same design -> same bytes
    header.SetOriginatingSystem(TCollection_HAsciiString("Wright (build123d / Open CASCADE)"))
    header.SetSchemaIdentifiersValue(1, TCollection_HAsciiString(AP242))
    Interface_Static.SetIVal_s("write.surfacecurve.mode", 1)
    writer.Transfer(doc, STEPControl_StepModelType.STEPControl_AsIs)
    if writer.Write(str(path)) != IFSelect_ReturnStatus.IFSelect_RetDone:
        raise RuntimeError(f"STEP write failed: {path}")


def hex_color(h: str) -> Color:
    h = h.lstrip("#")
    return Color(int(h[0:2], 16) / 255, int(h[2:4], 16) / 255, int(h[4:6], 16) / 255)


def trsf_to_rows(t: gp_Trsf) -> list:
    return [[t.Value(r, c) for c in range(1, 5)] for r in range(1, 4)]


def rows_to_location(rows) -> Location:
    t = gp_Trsf()
    (a, b, c, d), (e, f, g, h), (i, j, k, l) = rows
    t.SetValues(a, b, c, d, e, f, g, h, i, j, k, l)
    return Location(t)


def mass_props(shape: Shape, density: float) -> dict:
    """Volume (mm3), mass (kg), CG (mm) and inertia about the CG (kg m2), from
    the exact B-rep. OCC's MatrixOfInertia is about the centre of mass with
    unit density in mm^5; x density (kg/m3) x 1e-9 (mm3->m3) x 1e-6 (mm2->m2)."""
    p = GProp_GProps()
    BRepGProp.VolumeProperties_s(shape.wrapped, p)
    vol = p.Mass()
    c = p.CentreOfMass()
    m = p.MatrixOfInertia()
    k = density * 1e-15
    inertia = [[m.Value(r, cc) * k for cc in (1, 2, 3)] for r in (1, 2, 3)]
    pp = p.PrincipalProperties()
    moments = [x * k for x in pp.Moments()]
    axes = []
    for ax in (pp.FirstAxisOfInertia(), pp.SecondAxisOfInertia(), pp.ThirdAxisOfInertia()):
        axes.append([ax.X(), ax.Y(), ax.Z()])
    return dict(volume_mm3=vol, mass_kg=vol * density * 1e-9, cg_mm=[c.X(), c.Y(), c.Z()],
                inertia_kg_m2=inertia, principal_moments_kg_m2=moments, principal_axes=axes)


def face_types(shape: Shape) -> dict:
    out: dict[str, int] = {}
    for f in shape.faces():
        n = f.geom_type.name if hasattr(f.geom_type, "name") else str(f.geom_type)
        out[n] = out.get(n, 0) + 1
    return out


def build(part: str, params: dict, key: str, cache: str, material: str | None = None,
          density: float | None = None, stl: bool | None = None, kind: str | None = None) -> dict:
    """Build one part from its function and parameters into the cache:
    `<key>.brep` (exact B-rep, fast to reload), `<key>.step` (AP242, named,
    coloured), `<key>.stl` for printed parts, and `<key>.json` (this result)."""
    t0 = time.perf_counter()
    fn = library.load(part)
    meta = fn.__wright_part__
    full = library.check_params(part, params)
    shape = fn(**full)
    if hasattr(shape, "part") and not isinstance(shape, Shape):  # a BuildPart context
        shape = shape.part
    if not isinstance(shape, Shape):
        raise TypeError(f"{part} returned {type(shape).__name__}, not a build123d Part")
    resolved_kind = kind or meta["kind"]
    if resolved_kind not in library.KINDS:
        raise ValueError(f"unknown part kind {resolved_kind!r}")
    mat = material or meta["material"]
    rho = density if density is not None else MATERIALS[mat]["density"]
    finite_number(rho, "density", positive=True)
    name = part.split(".", 1)[1]
    shape.label = name
    shape.color = hex_color(meta["color"])
    build_ms = (time.perf_counter() - t0) * 1e3
    valid = BRepCheck_Analyzer(shape.wrapped).IsValid()
    if not valid or not shape.solids():
        raise ValueError(f"{part}: kernel did not produce a valid solid")
    props = mass_props(shape, rho)
    if not (math.isfinite(props["volume_mm3"]) and props["volume_mm3"] > 0):
        raise ValueError(f"{part}: built a shape with no volume ({props['volume_mm3']:.3g} mm3); is it a solid?")
    bb = shape.bounding_box()
    joints = {}
    for label, j in getattr(shape, "joints", {}).items():
        joints[label] = trsf_to_rows(j.relative_location.wrapped.Transformation())
    base = Path(cache) / key
    base.parent.mkdir(parents=True, exist_ok=True)
    BRepTools.Write_s(shape.wrapped, str(base) + ".brep")
    export_step_ap242(shape, str(base) + ".step")
    want_stl = resolved_kind == "printed" if stl is None else stl
    if want_stl:
        export_stl(shape, str(base) + ".stl", tolerance=STL_TOL_MM, angular_tolerance=STL_ANGLE_RAD)
    res = dict(key=key, part=part, params=full, material=mat, density_kg_m3=rho, kind=resolved_kind,
               color=meta["color"], version=meta["version"], valid=valid, faces=face_types(shape),
               n_faces=len(shape.faces()), n_edges=len(shape.edges()),
               bbox_mm=[[bb.min.X, bb.min.Y, bb.min.Z], [bb.max.X, bb.max.Y, bb.max.Z]],
               joints=joints, stl=want_stl, build_ms=build_ms,
               total_ms=(time.perf_counter() - t0) * 1e3, **props)
    Path(str(base) + ".json").write_text(json.dumps(res, indent=1))
    return res


# --------------------------------------------------------------------------------------
def _matmul(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)] for i in range(3)]


def _transpose(a):
    return [list(r) for r in zip(*a)]


def _bbox_gap(a, b) -> float:
    g = 0.0
    for k in range(3):
        d = max(a[0][k] - b[1][k], b[0][k] - a[1][k], 0.0)
        g = max(g, d)
    return g


def assemble(name: str, instances: list, out_dir: str, clearance_window_mm: float = 5.0,
             interference_tol_mm3: float = 1.0, contact_tol_mm: float = 0.1) -> dict:
    """Place every built instance, then check and export the assembly.

    `instances[i]`: {name, key, cache, placement, group, explode, color?, touches}
    (the Assembly JSON plus the cache key the Rust side assigned)."""
    t0 = time.perf_counter()
    validate_instances(instances)
    finite_number(clearance_window_mm, "clearance window", nonnegative=True)
    finite_number(interference_tol_mm3, "interference tolerance", nonnegative=True)
    finite_number(contact_tol_mm, "contact tolerance", nonnegative=True)
    clearance_window_mm = max(clearance_window_mm, contact_tol_mm)
    out = Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    built, world, placed = {}, {}, {}
    for inst in instances:
        base = Path(inst["cache"]) / inst["key"]
        metadata = json.loads(Path(str(base) + ".json").read_text())
        if metadata.get("valid") is not True:
            raise ValueError(f"{inst['name']}: cached part is invalid")
        finite_number(metadata["mass_kg"], "cached mass", positive=True)
        built[inst["name"]] = metadata
    contacts = set()
    for inst in instances:
        n = inst["name"]
        pl = inst["placement"]
        if "loc" in pl:
            loc = Location(tuple(pl["loc"]["pos"]), tuple(pl["loc"]["rot"]))
        else:
            m = pl["mate"]
            other, theirs, mine = m["to"], m["theirs"], m["mine"]
            if theirs not in built[other]["joints"]:
                raise KeyError(f"{n}: {other} has no joint {theirs!r}; has {sorted(built[other]['joints'])}")
            if mine not in built[n]["joints"]:
                raise KeyError(f"{n}: has no joint {mine!r}; has {sorted(built[n]['joints'])}")
            their_world = world[other] * rows_to_location(built[other]["joints"][theirs])
            loc = their_world * rows_to_location(built[n]["joints"][mine]).inverse()
            contacts.add(frozenset((n, other)))
        for t in inst.get("touches") or []:
            contacts.add(frozenset((n, t)))
        world[n] = loc
        shape = import_brep(str(Path(inst["cache"]) / inst["key"]) + ".brep")
        if not BRepCheck_Analyzer(shape.wrapped).IsValid() or not shape.solids():
            raise ValueError(f"{n}: cached B-rep is not a valid solid")
        s = shape.moved(loc)
        s.label = n
        s.color = hex_color(inst.get("color") or built[n]["color"])
        placed[n] = s
    names = [i["name"] for i in instances]
    # mass properties: rotate each part's inertia into the world, parallel-axis to the assembly CG
    total = sum(built[n]["mass_kg"] for n in names)
    cgs = {}
    for n in names:
        t = world[n].wrapped.Transformation()
        c = built[n]["cg_mm"]
        q = gp_Pnt(*c).Transformed(t)
        cgs[n] = [q.X(), q.Y(), q.Z()]
    cg = [sum(built[n]["mass_kg"] * cgs[n][k] for n in names) / total for k in range(3)] if total > 0 else [0, 0, 0]
    I = [[0.0] * 3 for _ in range(3)]
    for n in names:
        rows = trsf_to_rows(world[n].wrapped.Transformation())
        R = [r[:3] for r in rows]
        Iw = _matmul(_matmul(R, built[n]["inertia_kg_m2"]), _transpose(R))
        d = [(cgs[n][k] - cg[k]) * 1e-3 for k in range(3)]
        dd = sum(x * x for x in d)
        m = built[n]["mass_kg"]
        for i in range(3):
            for j in range(3):
                I[i][j] += Iw[i][j] + m * ((dd if i == j else 0.0) - d[i] * d[j])
    # all pairs: bounding-box broad phase, then exact B-rep common volume and distance
    boxes = {}
    for n, s in placed.items():
        bb = s.bounding_box()
        boxes[n] = [[bb.min.X, bb.min.Y, bb.min.Z], [bb.max.X, bb.max.Y, bb.max.Z]]
    clashes, near, floating, checked = [], [], [], 0
    for i, a in enumerate(names):
        for b in names[i + 1:]:
            pair = frozenset((a, b))
            gap = _bbox_gap(boxes[a], boxes[b])
            if gap > clearance_window_mm:
                if pair in contacts:
                    floating.append(dict(a=a, b=b, distance_mm=gap))
                continue
            checked += 1
            vol = 0.0
            if gap <= 0.0:
                op = BRepAlgoAPI_Common(placed[a].wrapped, placed[b].wrapped)
                if not op.IsDone():
                    raise RuntimeError(f"intersection failed for {a} / {b}")
                gp = GProp_GProps()
                BRepGProp.VolumeProperties_s(op.Shape(), gp)
                vol = gp.Mass()
            dist = BRepExtrema_DistShapeShape(placed[a].wrapped, placed[b].wrapped)
            if not dist.IsDone():
                raise RuntimeError(f"distance failed for {a} / {b}")
            kind, evidence = pair_result(a, b, vol, dist.Value(), pair in contacts,
                                         interference_tol_mm3, contact_tol_mm)
            if kind == "clash":
                clashes.append(evidence)
            elif kind == "floating":
                floating.append(evidence)
            elif kind == "clearance":
                near.append(evidence)
    near.sort(key=lambda r: r["clearance_mm"])
    # part tree from group paths, exploded offsets
    exploded = {}
    for inst in instances:
        n = inst["name"]
        if inst.get("explode") is not None:
            exploded[n] = list(inst["explode"])
        else:
            d = [cgs[n][k] - cg[k] for k in range(3)]
            exploded[n] = [x * 0.5 for x in d]
    tree: dict = {}
    for inst in instances:
        node = tree
        for g in [g for g in (inst.get("group") or "").split("/") if g]:
            node = node.setdefault(g, {})
        node[inst["name"]] = None

    def to_compound(label, node):
        kids = []
        for k, v in node.items():
            kids.append(placed[k] if v is None else to_compound(k, v))
        c = Compound(children=kids)
        c.label = label
        return c

    asm = to_compound(name, tree)
    t1 = time.perf_counter()
    export_step_ap242(asm, out / "assembly.step")
    export_gltf(asm, str(out / "assembly.glb"), unit=Unit.MM, binary=True,
                linear_deflection=GLB_TOL_MM, angular_deflection=0.3)
    edges = {}
    for n, s in placed.items():
        lines = []
        for e in s.edges():
            L = e.length
            k = 1 if e.geom_type.name == "LINE" else max(4, min(64, int(L / 2.0)))
            pts = [e.position_at(i / k) for i in range(k + 1)]
            lines.append([[round(p.X, 3), round(p.Y, 3), round(p.Z, 3)] for p in pts])
        edges[n] = lines
    (out / "edges.json").write_text(json.dumps({"unit": "mm", "parts": edges}))
    report = dict(
        name=name, n_parts=len(names), mass_kg=total, cg_mm=cg, inertia_kg_m2=I,
        bbox_mm=[[min(boxes[n][0][k] for n in names) for k in range(3)], [max(boxes[n][1][k] for n in names) for k in range(3)]] if names else None,
        parts={n: dict(part=built[n]["part"], group=next(i.get("group", "") for i in instances if i["name"] == n),
                       world=trsf_to_rows(world[n].wrapped.Transformation()), cg_world_mm=cgs[n],
                       mass_kg=built[n]["mass_kg"], volume_mm3=built[n]["volume_mm3"], material=built[n]["material"],
                       kind=built[n]["kind"], valid=built[n]["valid"], exploded_offset_mm=exploded[n],
                       bbox_world_mm=boxes[n]) for n in names},
        tree=tree, pairs_total=len(names) * (len(names) - 1) // 2, pairs_checked=checked,
        passed=not clashes and not floating, clashes=clashes, floating=floating, nearest=near[:20], contacts=[sorted(p) for p in contacts],
        place_check_ms=(t1 - t0) * 1e3, export_ms=(time.perf_counter() - t1) * 1e3,
    )
    (out / "assembly.json").write_text(json.dumps(report, indent=1))
    return report
