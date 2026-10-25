"""Native sketch sections and ruled lofts, with checked world-coordinate profiles."""
import FreeCAD as App
import Part
import Sketcher


def prepare(points, vector):
    if not isinstance(points,(list,tuple)) or len(points)<3:raise ValueError('profile needs at least three XYZ points')
    vertices=[vector(p) for p in points]
    if (vertices[-1]-vertices[0]).Length<1e-8:vertices.pop()
    if len(vertices)<3:raise ValueError('degenerate profile')
    if any((a-b).Length<1e-8 for a,b in zip(vertices,vertices[1:]+vertices[:1])):
        raise ValueError('profile has a zero-length edge')
    origin=vertices[0]
    x=vertices[1]-origin;x.normalize()
    normal=max((x.cross(v-origin) for v in vertices[2:]),key=lambda n:n.Length)
    if normal.Length<1e-8:raise ValueError('profile points are collinear')
    normal.normalize()
    if any(abs(normal.dot(v-origin))>1e-6 for v in vertices):raise ValueError('profile is not planar')
    y=normal.cross(x)
    face=Part.Face(Part.makePolygon(vertices+[vertices[0]]))
    if not face.isValid() or face.Area<=1e-8:raise ValueError('profile is self-intersecting or invalid')
    local=[App.Vector(x.dot(v-origin),y.dot(v-origin),0) for v in vertices]
    geometry=[Part.LineSegment(a,b) for a,b in zip(local,local[1:]+local[:1])]
    placement=App.Placement(origin,App.Rotation(x,y,normal,'ZXY'))
    return geometry,placement


def assign(sketch, points, vector):
    if getattr(sketch,'WrightSplineProfile',False):
        return assign_spline(sketch,points,sketch.WrightSplineSplitIndex,vector,
                             sketch.WrightSplineSampledTolerance)
    geometry,placement=prepare(points,vector)
    # Profiles are fully constrained by their supplied geometry; the typed
    # set_profile operation edits them transactionally in world coordinates.
    # Native sketches/constraints persist in FCStd without a Python proxy.
    for i in reversed(range(sketch.ConstraintCount)):sketch.delConstraint(i)
    for i in reversed(range(sketch.GeometryCount)):sketch.delGeometry(i)
    sketch.Placement=placement
    sketch.addGeometry(geometry,False)
    sketch.addConstraint([Sketcher.Constraint('Block',i) for i in range(len(geometry))])


def loft(doc, name, rings, vector):
    if doc.getObject(name):raise ValueError('loft name already exists')
    if not isinstance(rings,(list,tuple)) or len(rings)<2:raise ValueError('loft needs at least two profiles')
    if len({len(r) for r in rings})!=1:raise ValueError('loft sections need corresponding vertex counts')
    sections=[]
    for i,points in enumerate(rings):
        section_name=name+f'_Section{i}'
        if doc.getObject(section_name):raise ValueError('profile name already exists')
        sketch=doc.addObject('Sketcher::SketchObject',section_name)
        sketch.addProperty('App::PropertyBool','WrightProfile','Wright').WrightProfile=True
        assign(sketch,points,vector)
        sections.append(sketch)
    result=doc.addObject('Part::Loft',name)
    result.Sections=sections
    result.Solid=True
    result.Ruled=True
    result.Closed=False
    doc.recompute()
    return result


def extruded_skin(doc,name,points,direction,wall,vector):
    sketch=doc.addObject('Sketcher::SketchObject',name+'_Profile')
    sketch.addProperty('App::PropertyBool','WrightProfile','Wright').WrightProfile=True
    assign(sketch,points,vector)
    offset=doc.addObject('Part::Offset2D',name+'_InnerProfile')
    offset.addProperty('App::PropertyBool','WrightProfile','Wright').WrightProfile=True
    offset.Source=sketch;offset.Value=-wall;offset.Fill=False;offset.Join='Arc'
    axis=vector(direction)
    if axis.Length<=1e-8:raise ValueError('skin extrusion cannot be zero length')
    # Extrusion must leave the section plane, not sweep within its own face.
    normal=sketch.Placement.Rotation.multVec(App.Vector(0,0,1))
    if abs(normal.dot(axis))<=1e-8:raise ValueError('skin extrusion lies in its profile plane')
    extrusions=[]
    for suffix,base in [('_Outer',sketch),('_Inner',offset)]:
        obj=doc.addObject('Part::Extrusion',name+suffix)
        obj.Base=base;obj.DirMode='Custom';obj.Dir=axis;obj.LengthFwd=axis.Length;obj.Solid=True
        extrusions.append(obj)
    extrusions[1].setExpression('LengthFwd',extrusions[0].Name+'.LengthFwd')
    doc.recompute()
    if offset.Shape.isNull() or len(offset.Shape.Wires)!=1 or not offset.Shape.isValid():
        raise ValueError('wall offset did not produce one valid cavity profile')
    cut=doc.addObject('Part::Cut',name)
    cut.Base,cut.Tool=extrusions
    doc.recompute()
    if cut.Shape.isNull() or cut.Shape.Volume>=extrusions[0].Shape.Volume-1e-6:
        raise ValueError('skin construction did not remove a cavity')
    return cut


def _polyline_distances(points, vertices):
    """Exact Euclidean distance from supplied points to closed straight segments."""
    import math
    segments=[]
    for a,b in zip(vertices,vertices[1:]+vertices[:1]):
        dx,dy,dz=b.x-a.x,b.y-a.y,b.z-a.z
        length2=dx*dx+dy*dy+dz*dz
        if length2<=0:raise ValueError('zero length source polyline segment')
        segments.append((a.x,a.y,a.z,dx,dy,dz,length2))
    distances=[]
    for p in points:
        closest=float('inf')
        for ax,ay,az,dx,dy,dz,length2 in segments:
            t=max(0.0,min(1.0,((p.x-ax)*dx+(p.y-ay)*dy+(p.z-az)*dz)/length2))
            distance2=(p.x-ax-t*dx)**2+(p.y-ay-t*dy)**2+(p.z-az-t*dz)**2
            closest=min(closest,distance2)
        distances.append(math.sqrt(closest))
    return distances


def _spline_data(points, split_index, vector, sampled_tolerance_mm):
    """Preflight in the profile's local frame, before mutating native objects."""
    import math
    if type(split_index) is not int:raise ValueError('spline split_index must be an integer')
    if (isinstance(sampled_tolerance_mm,bool) or not isinstance(sampled_tolerance_mm,(int,float))
            or not math.isfinite(sampled_tolerance_mm) or not 0<sampled_tolerance_mm<=1):
        raise ValueError('sampled_tolerance_mm must be finite and in (0,1] mm')
    if not isinstance(points,(list,tuple)) or not 8<=len(points)<=257:
        raise ValueError('spline section requires 8 to 256 unique boundary points')
    for point in points:
        vertex=vector(point)
        if max(abs(vertex.x),abs(vertex.y),abs(vertex.z))>1e6:
            raise ValueError('spline coordinates must stay within +/-1e6 mm')
    geometry,placement=prepare(points,vector)
    local=[edge.StartPoint for edge in geometry]
    if not 8<=len(local)<=256 or not 3<=split_index<=len(local)-3:
        raise ValueError('two spline branches each require at least four points')
    branches=[local[:split_index+1],local[split_index:]+local[:1]]
    curves=[];residuals=[];deviations=[]
    samples_per_curve=201
    for branch in branches:
        curve=Part.BSplineCurve();curve.interpolate(branch)
        edge=curve.toShape()
        residuals.extend(Part.Vertex(p).distToShape(edge)[0] for p in branch)
        deviations.extend(_polyline_distances(curve.discretize(Number=samples_per_curve),local))
        curves.append(curve)
    wire=Part.Wire([curve.toShape() for curve in curves])
    face=Part.Face(wire)
    if not wire.isClosed() or not face.isValid() or face.Area<=1e-8:
        raise ValueError('interpolated spline profile is not a valid closed face')
    if any(not math.isfinite(v) for v in residuals+deviations):
        raise ValueError('nonfinite spline deviation evidence')
    if max(residuals)>1e-6:raise ValueError('spline did not interpolate source coordinates')
    if max(deviations)>sampled_tolerance_mm:
        raise ValueError('sampled spline deviation exceeds declared tolerance')
    evidence=dict(source_unique_points=len(local),source_point_fit_max_mm=max(residuals),
        sampled_curve_to_source_polyline_max_mm=max(deviations),samples_per_curve=samples_per_curve,
        sampled_tolerance_mm=float(sampled_tolerance_mm),certified_hausdorff_bound=False,
        method='Exact point-to-curve distance at every input coordinate; 201 native curve samples per branch measured against source closed polyline. The sampled maximum is not a bound between samples.',
        coordinate_frame='Sketch local XY; native Placement and expression-driven section positions remain separate',
        curve_count=2,curve_degrees=[c.Degree for c in curves],split_index=split_index)
    return curves,placement,local,evidence


def _apply_spline(sketch, data):
    import json
    curves,placement,local,evidence=data
    expressions=list(sketch.ExpressionEngine)
    for i in reversed(range(sketch.ConstraintCount)):sketch.delConstraint(i)
    for i in reversed(range(sketch.GeometryCount)):sketch.delGeometry(i)
    sketch.Placement=placement
    sketch.addGeometry(curves,False)
    sketch.addConstraint([Sketcher.Constraint('Block',i) for i in range(2)])
    for prop,expression in expressions:sketch.setExpression(prop,expression)
    properties=[('App::PropertyBool','WrightProfile',True),
        ('App::PropertyBool','WrightSplineProfile',True),
        ('App::PropertyInteger','WrightSplineSplitIndex',evidence['split_index']),
        ('App::PropertyFloat','WrightSplineSampledTolerance',evidence['sampled_tolerance_mm']),
        ('App::PropertyString','WrightSplineSourceLocalPoints',json.dumps([[p.x,p.y,p.z] for p in local])),
        ('App::PropertyString','WrightSplineEvidence',json.dumps(evidence))]
    for kind,key,value in properties:
        if key not in sketch.PropertiesList:sketch.addProperty(kind,key,'WrightSpline')
        setattr(sketch,key,value);sketch.setEditorMode(key,1)
    return evidence


def assign_spline(sketch, points, split_index, vector, sampled_tolerance_mm=0.2):
    """Replace one tagged native section; callers own the edit transaction."""
    import json
    data=_spline_data(points,split_index,vector,sampled_tolerance_mm)
    if getattr(sketch,'WrightSplineProfile',False):
        if (split_index!=sketch.WrightSplineSplitIndex
                or len(data[2])!=len(json.loads(sketch.WrightSplineSourceLocalPoints))):
            raise ValueError('spline edits must preserve source point count and branch split')
    return _apply_spline(sketch,data)


def spline_loft(doc, name, rings, split_index, vector, sampled_tolerance_mm=0.2, ruled=True):
    """Two-curve native sections; sampled fit gate, never exact polygon equality.

    Geometry and provenance persist without Python proxies. Validation happens
    before creation; construction failures remove only newly created objects.
    The typed caller may wrap this helper in its existing document transaction.
    """
    import json,re
    if not isinstance(ruled,bool):
        raise ValueError('ruled must be a boolean')
    if not isinstance(name,str) or not re.fullmatch(r'[A-Za-z][A-Za-z0-9_]{0,95}',name):
        raise ValueError('invalid spline loft name')
    if not isinstance(rings,(list,tuple)) or not 2<=len(rings)<=16:
        raise ValueError('spline loft requires 2 to 16 sections')
    if any(not isinstance(r,(list,tuple)) for r in rings) or sum(map(len,rings))>2048:
        raise ValueError('spline loft input exceeds 2048 total points')
    names=[name]+[name+f'_Section{i}' for i in range(len(rings))]
    if any(len(n)>96 for n in names):raise ValueError('spline result name leaves no room for section names')
    if any(doc.getObject(n) for n in names):raise ValueError('spline result or section name already exists')
    prepared=[_spline_data(r,split_index,vector,sampled_tolerance_mm) for r in rings]
    if len({len(data[2]) for data in prepared})!=1:
        raise ValueError('spline sections require corresponding source point counts')
    created=[]
    try:
        sections=[]
        for i,data in enumerate(prepared):
            sketch=doc.addObject('Sketcher::SketchObject',names[i+1]);created.append(sketch.Name)
            _apply_spline(sketch,data);sections.append(sketch)
        obj=doc.addObject('Part::Loft',name);created.append(obj.Name)
        obj.Sections=sections;obj.Solid=True;obj.Ruled=ruled;obj.Closed=False
        obj.addProperty('App::PropertyString','WrightSplineRepresentation','WrightSpline')
        obj.WrightSplineRepresentation=json.dumps(dict(kind='two_interpolating_bspline_sections',
            source_section_count=len(sections),split_index=split_index,
            sampled_tolerance_mm=sampled_tolerance_mm,ruled=ruled,
            loft_mode='ruled' if ruled else 'smooth',exact_polygon_equivalence=False))
        obj.setEditorMode('WrightSplineRepresentation',1)
        doc.recompute()
        if any(not s.FullyConstrained or not s.Shape.isClosed() for s in sections):
            raise ValueError('spline sections must remain closed and fully constrained')
        if obj.Shape.isNull() or not obj.Shape.isValid() or len(obj.Shape.Solids)!=1 or obj.Shape.Volume<=0:
            raise ValueError('spline loft did not form one valid solid')
        return obj
    except Exception:
        for n in reversed(created):doc.removeObject(n)
        doc.recompute();raise
