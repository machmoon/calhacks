"""A rectangular block from (0,0,0) to (x, y, z): stand-ins for bought
boxes (batteries, boards, cameras) until their own models exist."""
from typing import Annotated as A

from build123d import Align, Box, Location, Part, RigidJoint

from wright_cad import Param, part


@part(material="pcb_fr4", color="#3a3f44", kind="bought")
def block(
    x: A[float, Param("mm", "size along X", min=0.1, max=5000)] = 50.0,
    y: A[float, Param("mm", "size along Y", min=0.1, max=5000)] = 30.0,
    z: A[float, Param("mm", "size along Z", min=0.1, max=5000)] = 10.0,
) -> Part:
    """Joints: `origin` at the min corner, `top` at the top face's centre."""
    b = Box(x, y, z, align=(Align.MIN, Align.MIN, Align.MIN))
    RigidJoint("origin", b, Location((0, 0, 0)))
    RigidJoint("top", b, Location((x / 2, y / 2, z)))
    return b
