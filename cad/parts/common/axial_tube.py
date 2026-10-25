"""Tube or rod between two points, preserving the domain's common coordinate frame."""
from typing import Annotated as A
import math
from build123d import Plane, Solid
from wright_cad import Param, part


@part(material="carbon_fiber", kind="bought", color="#353941")
def axial_tube(
    start: A[list, Param("mm", "first endpoint XYZ")] = (0,0,0),
    end: A[list, Param("mm", "second endpoint XYZ")] = (0,0,100),
    radius: A[float, Param("mm", "outer radius", min=.01, max=1000)] = 10.,
    bore_radius: A[float, Param("mm", "bore radius; zero for solid", min=0, max=1000)] = 8.,
):
    if len(start) != 3 or len(end) != 3:
        raise ValueError("endpoints must be XYZ vectors")
    delta = [b-a for a,b in zip(start,end)]
    length = math.sqrt(sum(v*v for v in delta))
    if length <= 0 or bore_radius >= radius:
        raise ValueError("tube needs positive length and bore smaller than radius")
    plane = Plane(origin=start,z_dir=delta)
    outer = Solid.make_cylinder(radius,length,plane)
    return outer if bore_radius == 0 else outer.cut(Solid.make_cylinder(bore_radius,length,plane))
