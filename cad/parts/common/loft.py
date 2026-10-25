"""Closed polygonal sections lofted as B-rep solids, with optional cavity sections."""
from typing import Annotated as A
from build123d import Solid, Wire
from wright_cad import Param, part


def _remove_linear_sections(sections, tolerance=1e-8):
    """Remove only sections whose corresponding vertices lie on the ruled span.

    This preserves the surface within tolerance in mm. It avoids six identical
    sets of side faces for a straight wing, without smoothing the source foil.
    """
    result = list(sections)
    index = 1
    while index < len(result)-1:
        a, mid, b = result[index-1:index+2]
        if len(a) != len(mid) or len(a) != len(b):
            index += 1
            continue
        flat_a = [v for point in a for v in point]
        flat_b = [v for point in b for v in point]
        flat_m = [v for point in mid for v in point]
        axis = max(range(len(flat_a)), key=lambda k: abs(flat_b[k]-flat_a[k]))
        delta = flat_b[axis]-flat_a[axis]
        if abs(delta) < tolerance:
            index += 1
            continue
        t = (flat_m[axis]-flat_a[axis])/delta
        if 0 < t < 1 and all(abs(m-(x+t*(y-x))) <= tolerance for x,m,y in zip(flat_a,flat_m,flat_b)):
            result.pop(index)
        else:
            index += 1
    return result


@part(material="pla", kind="printed", color="#e8e4da")
def loft(
    rings: A[list, Param("mm", "sections: each a closed loop of XYZ points")] = (((0,0,0),(10,0,0),(10,10,0),(0,10,0)),((0,0,20),(10,0,20),(10,10,20),(0,10,20))),
    inner: A[list, Param("mm", "optional cavity sections, subtracted from outer solid")] = (),
):
    def solid(sections):
        if len(sections) < 2:
            raise ValueError("loft needs at least two sections")
        wires = []
        for ring in _remove_linear_sections(sections):
            if len(ring) < 3 or any(len(p) != 3 for p in ring):
                raise ValueError("each section needs at least three XYZ points")
            wires.append(Wire.make_polygon(ring, close=True))
        return Solid.make_loft(wires, ruled=True)
    outer = solid(rings)
    if not inner:
        return outer
    cavity = solid(inner)
    result = outer.cut(cavity)
    if len(result.solids()) != 1:
        raise ValueError("shell must be one connected solid")
    return result
