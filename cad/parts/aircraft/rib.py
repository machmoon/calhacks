"""Airfoil-section rib with exact spar bores. X is chord; Y is airfoil height."""
from typing import Annotated as A
import math
from build123d import BuildPart, BuildSketch, Circle, Locations, Mode, Polygon, extrude
from wright_cad import Param, part


@part(material="petg", kind="printed", color="#d5a64b")
def rib(
    profile: A[list, Param("x/c,y/c", "closed airfoil coordinates in order")] = ((0,0),(.5,.1),(1,0),(.5,-.1)),
    chord: A[float, Param("mm", "rib chord", min=10,max=2000)] = 210.,
    thickness: A[float, Param("mm", "rib thickness", min=.5,max=30)] = 2.,
    bores: A[list, Param("mm", "spar bores as [x,y,radius] in rib plane")] = ((58.8,0,6.35),(136.5,0,2.)),
):
    points = [(float(x)*chord,float(y)*chord) for x,y in profile]
    if points[-1] == points[0]:
        points.pop()
    with BuildPart() as blank:
        with BuildSketch():
            Polygon(*points, align=None)
        extrude(amount=thickness)
    result = blank.part
    if not result.is_valid:
        raise ValueError("airfoil profile does not form a valid solid; check crossings and closure")
    for x,y,radius in bores:
        if radius <= 0 or not math.isfinite(radius):
            raise ValueError("spar bore radius must be positive and finite")
        with BuildPart() as cutter:
            with BuildSketch():
                with Locations((x,y)):
                    Circle(radius)
            extrude(amount=thickness)
        new = result.cut(cutter.part)
        removed = result.volume-new.volume
        expected = math.pi*radius*radius*thickness
        if abs(removed-expected) > expected*1e-6:
            raise ValueError("spar bore exits the rib or overlaps another bore")
        result = new
    if len(result.solids()) != 1:
        raise ValueError("spar bores disconnect the rib")
    return result
