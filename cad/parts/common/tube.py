"""A round tube along +Z from z=0: airframe bays, tank shells, spars, booms."""
from typing import Annotated as A

from build123d import Axis, BuildPart, BuildSketch, Circle, Location, Mode, Part, RigidJoint, extrude

from wright_cad import Param, part


@part(material="al6061_t6", color="#b8bcc2", kind="bought")
def tube(
    od: A[float, Param("mm", "outer diameter", min=0.5, max=2000)] = 25.0,
    wall: A[float, Param("mm", "wall thickness; >= od/2 makes a solid rod", min=0.05, max=1000)] = 1.0,
    length: A[float, Param("mm", "length along +Z", min=0.1, max=20000)] = 100.0,
) -> Part:
    """Joints: `bottom` (z=0) and `top` (z=length), both facing +Z."""
    with BuildPart() as p:
        with BuildSketch():
            Circle(od / 2)
            if wall < od / 2:
                Circle(od / 2 - wall, mode=Mode.SUBTRACT)
        extrude(amount=length)
    t = p.part
    RigidJoint("bottom", t, Location((0, 0, 0)))
    RigidJoint("top", t, Location((0, 0, length)))
    return t
