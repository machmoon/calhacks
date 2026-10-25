"""A flat plate cut to a polygon outline (XY, mm), extruded along +Z: fins,
brackets, shelves, laser-cut or CNC'd sheet."""
from typing import Annotated as A

from build123d import BuildLine, BuildPart, BuildSketch, Location, Part, Polyline, RigidJoint, extrude, make_face

from wright_cad import Param, part


@part(material="g10_fr4", color="#c8d29a", kind="cut")
def plate(
    outline: A[list, Param("mm", "[[x, y], ...] closed outline in XY, no repeated end point")] = ((0, 0), (100, 0), (100, 50), (0, 50)),
    thickness: A[float, Param("mm", "sheet thickness along +Z", min=0.1, max=200)] = 3.0,
) -> Part:
    """Joints: `origin` at (0, 0, 0)."""
    pts = [(float(x), float(y)) for x, y in outline]
    with BuildPart() as p:
        with BuildSketch():
            with BuildLine():
                Polyline(*pts, close=True)
            make_face()
        extrude(amount=thickness)
    s = p.part
    RigidJoint("origin", s, Location((0, 0, 0)))
    return s
