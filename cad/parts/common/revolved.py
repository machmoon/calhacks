"""A solid of revolution about +Z from an (r, z) profile: nose cones,
chambers, nozzles, bottles. The profile is the outline in the XZ half-plane
(r >= 0), listed from one end to the other; it is closed along the axis."""
from typing import Annotated as A

from build123d import Axis, BuildLine, BuildPart, BuildSketch, Location, Part, Plane, Polyline, RigidJoint, make_face, revolve

from wright_cad import Param, part


@part(material="al6061_t6", color="#b8bcc2", kind="machined")
def revolved(
    profile: A[list, Param("mm", "[[r, z], ...] outline from one end to the other, r >= 0")] = ((0, 0), (10, 0), (10, 50), (0, 50)),
) -> Part:
    """Joints: `bottom` at the lowest z on the axis, `top` at the highest."""
    pts = [(float(r), float(z)) for r, z in profile]
    if len(pts) < 3 or any(r < 0 for r, _ in pts):
        raise ValueError("profile needs >= 3 points with r >= 0")
    ring = [(r, 0.0, z) for r, z in pts]
    if ring[0][0] > 0:
        ring.insert(0, (0.0, 0.0, ring[0][2]))
    if ring[-1][0] > 0:
        ring.append((0.0, 0.0, ring[-1][2]))
    dedup = [ring[0]] + [q for a, q in zip(ring, ring[1:]) if q != a]
    with BuildPart() as p:
        with BuildSketch(Plane.XZ):
            with BuildLine(Plane.XY):
                Polyline(*dedup, close=True)
            make_face()
        revolve(axis=Axis.Z)
    s = p.part
    zs = [z for _, z in pts]
    RigidJoint("bottom", s, Location((0, 0, min(zs))))
    RigidJoint("top", s, Location((0, 0, max(zs))))
    return s
