"""A flat round plate along +Z from z=0: bulkheads, centering rings, shelves."""
from typing import Annotated as A

from build123d import BuildPart, BuildSketch, Circle, Location, Mode, Part, RigidJoint, extrude

from wright_cad import Param, part


@part(material="al6061_t6", color="#a9adb3", kind="machined")
def disc(
    od: A[float, Param("mm", "outer diameter", min=1, max=3000)] = 100.0,
    thickness: A[float, Param("mm", "thickness along +Z", min=0.1, max=500)] = 6.0,
    bore: A[float, Param("mm", "central hole diameter; 0 for none", min=0, max=2999)] = 0.0,
) -> Part:
    """Joints: `bottom` (z=0) and `top` (z=thickness)."""
    with BuildPart() as p:
        with BuildSketch():
            Circle(od / 2)
            if bore > 0:
                Circle(bore / 2, mode=Mode.SUBTRACT)
        extrude(amount=thickness)
    d = p.part
    RigidJoint("bottom", d, Location((0, 0, 0)))
    RigidJoint("top", d, Location((0, 0, thickness)))
    return d
