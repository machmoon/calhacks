"""Read-only solid containment and clearance to every container boundary."""
import hashlib
import math
import Part


def _solid_leaves(shape):
    if shape.ShapeType == 'Solid':
        return 1
    if shape.ShapeType not in ('Compound', 'CompSolid'):
        raise ValueError('reference surfaces/edges are not physical containment solids')
    children = shape.childShapes()
    if not children:
        raise ValueError('empty containment geometry')
    return sum(_solid_leaves(child) for child in children)


def check(doc, container, contents, minimum_clearance_mm=0):
    if not isinstance(container, str) or not container:
        raise ValueError('container must be a native object name')
    if (not isinstance(contents, list) or not 1 <= len(contents) <= 32 or
            any(not isinstance(n, str) or not n for n in contents) or
            len(set(contents)) != len(contents) or container in contents):
        raise ValueError('contents must be 1..32 distinct names excluding container')
    if (isinstance(minimum_clearance_mm, bool) or
            not isinstance(minimum_clearance_mm, (int, float)) or
            not math.isfinite(minimum_clearance_mm) or
            not 0 <= minimum_clearance_mm <= 1000):
        raise ValueError('minimum clearance must be finite and 0..1000 mm')
    shapes = {}
    fingerprints = {}
    for name in [container] + contents:
        obj = doc.getObject(name)
        if obj is None or not hasattr(obj, 'Shape'):
            raise ValueError('missing native solid: ' + name)
        shape = obj.Shape.copy()
        # A compound containing one solid plus reference geometry is not a solid.
        if _solid_leaves(shape) != 1 or len(shape.Solids) != 1 or not shape.isValid():
            raise ValueError('containment requires one valid solid per object: ' + name)
        if len(shape.Faces) > 100000:
            raise ValueError('containment face budget exceeded')
        fingerprints[name] = hashlib.sha256(shape.cleaned().exportBrepToString().encode()).hexdigest()
        shapes[name] = shape
    enclosing = shapes[container]
    boundary = Part.makeCompound(enclosing.Shells)
    results = []
    for name in contents:
        shape = shapes[name]
        outside = shape.cut(enclosing)
        if not outside.isNull() and not outside.isValid():
            raise ValueError('invalid containment Boolean result: ' + name)
        outside_volume = outside.Volume
        distance, points, _ = shape.distToShape(boundary)
        if not all(math.isfinite(v) and v >= 0 for v in (outside_volume, distance)):
            raise ValueError('nonfinite or negative containment evidence')
        contained = outside_volume <= 1e-6
        clearance_passed = contained and distance + 1e-7 >= minimum_clearance_mm
        results.append(dict(object=name, contained=contained,
            outside_volume_mm3=outside_volume, boundary_distance_mm=distance,
            clearance_passed=clearance_passed,
            closest_point_pairs=[[[a.x,a.y,a.z],[b.x,b.y,b.z]] for a,b in points[:8]]))
    return dict(passed=all(row['clearance_passed'] for row in results),
        container=container, results=results, minimum_clearance_mm=minimum_clearance_mm,
        volume_tolerance_mm3=1e-6, distance_tolerance_mm=1e-7,
        geometry_sha256=fingerprints,
        scope='Static containment in the named solid and distance to all its boundary shells. '
              'For wall thickness use the outer solid as container and the cavity solid as content. '
              'For cavity fit use the cavity solid as container. No checks between contents, '
              'motion, strength, complete shell construction or manufacturing approval.')
