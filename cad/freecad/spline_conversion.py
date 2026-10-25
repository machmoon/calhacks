"""Bounded polygon-section to spline-loft conversion with snapshot provenance."""
import hashlib
import json
import re
import FreeCAD as App
import Part
import layout
import native
import profiles

_NUMBER=r'[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?'
_LINEAR=re.compile(r'([A-Za-z][A-Za-z0-9_]*)\.([A-Za-z][A-Za-z0-9_]*)(?:\s*\*\s*('+_NUMBER+r'))?(?:\s*\+\s*\(?\s*('+_NUMBER+r')\s*mm\s*\)?)?')


def geometry_hash(shape):
    return hashlib.sha256(shape.cleaned().exportBrepToString().encode()).hexdigest()


def _same_placement(a,b):
    return (a.Base-b.Base).Length<1e-7 and abs(a.Rotation.multiply(b.Rotation.inverted()).Angle)<1e-7


def _section(doc,sketch):
    if sketch.TypeId!='Sketcher::SketchObject' or not getattr(sketch,'WrightProfile',False) or getattr(sketch,'WrightSplineProfile',False):
        raise ValueError('conversion requires polygon Wright sketch sections')
    if not 8<=sketch.GeometryCount<=256:raise ValueError('conversion sections require 8 to 256 line edges')
    if not sketch.FullyConstrained or not _same_placement(sketch.Placement,sketch.getGlobalPlacement()):
        raise ValueError('conversion requires fully constrained sections without transformed parent containers')
    geometry=list(sketch.Geometry)
    for i,edge in enumerate(geometry):
        if not isinstance(edge,Part.LineSegment) or sketch.getConstruction(i):
            raise ValueError('conversion supports boundary line segments only')
        following=geometry[(i+1)%len(geometry)]
        if not isinstance(following,Part.LineSegment) or (edge.EndPoint-following.StartPoint).Length>1e-7:
            raise ValueError('section line geometry must be an ordered closed boundary')
    placement=sketch.getGlobalPlacement()
    points=[placement.multVec(e.StartPoint) for e in geometry]
    expressions=[]
    for prop,expression in sketch.ExpressionEngine:
        path=prop.lstrip('.')
        match=_LINEAR.fullmatch(expression.strip())
        if path not in {'Placement.Base.x','Placement.Base.y','Placement.Base.z'} or not match:
            raise ValueError('only simple shared-dimension translation expressions can be preserved')
        source=doc.getObject(match[1])
        if source is None or not layout.is_parameters(source) or match[2] not in layout.limits(source):
            raise ValueError('section expression must reference a declared shared dimension')
        if geometry[0].StartPoint.Length>1e-7:
            raise ValueError('expression preservation requires the Wright section origin at its first point')
        expressions.append((path,expression))
    xyz=[[p.x,p.y,p.z] for p in points]
    return xyz,expressions


def convert(doc,source,name,split_index,sampled_tolerance_mm):
    source=native.name(source);name=native.name(name)
    obj=doc.getObject(source)
    if obj is None or obj.TypeId!='Part::Loft' or not obj.Solid or not obj.Ruled or obj.Closed:
        raise ValueError('source must be an open-span solid ruled native Part::Loft')
    if not _same_placement(obj.getGlobalPlacement(),App.Placement()):
        raise ValueError('source loft placement must be identity; transformed lofts are unsupported')
    sections=list(obj.Sections)
    if not 2<=len(sections)<=16:raise ValueError('conversion requires 2 to 16 sections')
    if sum(s.GeometryCount for s in sections if hasattr(s,'GeometryCount'))>2048:
        raise ValueError('conversion exceeds 2048 total section points')
    # profiles.spline_loft also checks all result/section names and fit bounds.
    if doc.getObject(name):raise ValueError('conversion result already exists')
    native.describe(obj)
    extracted=[_section(doc,s) for s in sections]
    rings=[data[0] for data in extracted]
    source_hash=geometry_hash(obj.Shape)
    snapshot_hash=hashlib.sha256(json.dumps(rings,allow_nan=False,separators=(',',':')).encode()).hexdigest()
    doc.openTransaction('Convert '+source+' to spline loft')
    try:
        result=profiles.spline_loft(doc,name,rings,split_index,native.vector,sampled_tolerance_mm)
        preserved=[]
        for old,new,(_,expressions) in zip(sections,result.Sections,extracted):
            for prop,expression in expressions:
                new.setExpression(prop,expression)
                preserved.append(dict(source_section=old.Name,result_section=new.Name,property=prop,expression=expression))
        doc.recompute()
        for old,new in zip(sections,result.Sections):
            if (old.getGlobalPlacement().Base-new.getGlobalPlacement().Base).Length>1e-6:
                raise ValueError('preserved expressions changed section placement at conversion')
        fits=[json.loads(section.WrightSplineEvidence) for section in result.Sections]
        fit_evidence=dict(source_point_fit_max_mm=max(f['source_point_fit_max_mm'] for f in fits),
            sampled_curve_to_source_polyline_max_mm=max(f['sampled_curve_to_source_polyline_max_mm'] for f in fits),
            sampled_tolerance_mm=sampled_tolerance_mm,samples_per_curve=201,
            certified_hausdorff_bound=False,exact_polygon_equivalence=False)
        provenance=dict(source_object=source,source_geometry_sha256=source_hash,
            world_coordinate_snapshot_sha256=snapshot_hash,
            source_sections=[s.Name for s in sections],source_point_count=sum(map(len,rings)),
            preserved_layout_expressions=preserved,source_point_edits_associative=False,fit_evidence=fit_evidence,
            scope='Coordinate snapshot with copied shared-dimension translation expressions. Later source profile-point edits do not update this spline loft. Sampled deviation is not a certified continuous bound.')
        result.addProperty('App::PropertyString','WrightSplineConversion','WrightSpline')
        result.WrightSplineConversion=json.dumps(provenance,allow_nan=False)
        result.setEditorMode('WrightSplineConversion',1)
        validation=native.validate_document(doc)
        if geometry_hash(obj.Shape)!=source_hash:raise ValueError('conversion unexpectedly changed source geometry')
        evidence=native.describe(result)
        evidence.update(conversion=provenance,validated_object_count=len(validation),assembly_contacts_checked=False)
        doc.commitTransaction()
        return evidence
    except Exception:
        doc.abortTransaction();doc.recompute();raise
