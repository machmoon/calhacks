"""Rectangular mounting blank with explicitly specified cylindrical seats/bores.

Cutters are local XYZ endpoints plus radius. A cutter may open a saddle at an
edge; it must remove material and leave one connected valid solid. These seats
are geometric interfaces, not a bolt pattern or a strength certification.
"""
from typing import Annotated as A
import math
from build123d import Align, Box, Location, Plane, RigidJoint, Solid
from wright_cad import Param, part


@part(material="petg", kind="printed", color="#5c7fa3")
def mount_block(
    x: A[float, Param("mm","blank length",min=.1,max=2000)] = 40.,
    y: A[float, Param("mm","blank width",min=.1,max=2000)] = 20.,
    z: A[float, Param("mm","blank height",min=.1,max=2000)] = 20.,
    bores: A[list, Param("mm","[[start XYZ, end XYZ, radius], ...] seats in local coordinates")] = (((-1,10,10),(41,10,10),6.35),),
):
    shape=Box(x,y,z,align=(Align.MIN,Align.MIN,Align.MIN))
    joints=[]
    for index,(start,end,radius) in enumerate(bores):
        if len(start)!=3 or len(end)!=3 or not 0 < radius:
            raise ValueError("seat needs XYZ endpoints and positive radius")
        delta=[b-a for a,b in zip(start,end)]
        length=math.sqrt(sum(v*v for v in delta))
        if length<=0:
            raise ValueError("seat axis has zero length")
        cutter=Solid.make_cylinder(radius,length,Plane(origin=start,z_dir=delta))
        candidate=shape.cut(cutter)
        if not candidate.is_valid or len(candidate.solids())!=1:
            raise ValueError("seat disconnects or invalidates the mount")
        if shape.volume-candidate.volume<1e-6:
            raise ValueError("seat does not intersect the mount")
        shape=candidate
        joints.append((f"seat_{index}",Plane(origin=start,z_dir=delta).location))
    RigidJoint("origin",shape,Location())
    for name,location in joints:
        RigidJoint(name,shape,location)
    return shape
