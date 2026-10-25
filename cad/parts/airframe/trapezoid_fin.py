"""A sheet fin with an optional through-wall mounting tab, in XY, +Z thickness."""
from typing import Annotated as A
from build123d import BuildPart, BuildSketch, Polygon, Location, RigidJoint, extrude
from wright_cad import Param, part


@part(material="g10_fr4", kind="cut", color="#cf904b")
def trapezoid_fin(
    root: A[float, Param("mm", "root chord", min=1, max=2000)] = 150.,
    tip: A[float, Param("mm", "tip chord", min=0.1, max=2000)] = 90.,
    span: A[float, Param("mm", "exposed radial span", min=1, max=2000)] = 75.,
    sweep: A[float, Param("mm", "tip leading edge offset from root leading edge", min=-2000, max=2000)] = 44.,
    thickness: A[float, Param("mm", "sheet thickness", min=.1, max=100)] = 2.4,
    tab_length: A[float, Param("mm", "mounting tab chord; zero omits tab", min=0, max=2000)] = 0.,
    tab_height: A[float, Param("mm", "mounting tab depth below root", min=0, max=1000)] = 0.,
    tab_offset: A[float, Param("mm", "tab leading edge from root leading edge", min=0, max=2000)] = 0.,
):
    if (tab_length == 0) != (tab_height == 0):
        raise ValueError("tab length and height must both be zero or both positive")
    if tab_offset + tab_length > root + 1e-8:
        raise ValueError("mounting tab must lie within the root chord")
    points = [(0,0), (root,0), (sweep+tip,span), (sweep,span)]
    if tab_length:
        points = [(0,0),(tab_offset,0),(tab_offset,-tab_height),
                  (tab_offset+tab_length,-tab_height),(tab_offset+tab_length,0),
                  (root,0),(sweep+tip,span),(sweep,span)]
    # Tabs may start at 0 or end at the root's trailing edge.
    clean = []
    for point in points:
        if not clean or point != clean[-1]:
            clean.append(point)
    with BuildPart() as model:
        with BuildSketch():
            Polygon(*clean, align=None)
        extrude(amount=thickness)
    result = model.part
    RigidJoint("root", result, Location((0,0,thickness/2)))
    return result
