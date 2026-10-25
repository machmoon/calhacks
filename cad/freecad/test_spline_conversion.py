"""Native conversion boundaries plus actual reference-wing layout and snapshot checks."""
import hashlib,json,math
from pathlib import Path
import FreeCAD as App
import native,worker,spline_conversion
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'out/freecad-spline-conversion';OUT.mkdir(parents=True,exist_ok=True)
worker.document('ConversionBoundary');doc=worker.active()
rings=[[[10*math.cos(i*math.tau/12),5*math.sin(i*math.tau/12),z] for i in range(12)] for z in [0,20]]
native.make_part(doc,dict(name='Polygon',kind='loft',params=dict(rings=rings)))
native.make_part(doc,dict(name='Box',kind='box',params=dict(x=2,y=2,z=2)))
def reject(**kwargs):
    before={o.Name for o in doc.Objects};source=spline_conversion.geometry_hash(doc.Polygon.Shape)
    try:worker.METHODS['convert_spline_loft'](**kwargs)
    except ValueError:pass
    else:raise AssertionError('unsupported conversion accepted '+str(kwargs))
    assert {o.Name for o in doc.Objects}==before
    assert spline_conversion.geometry_hash(doc.Polygon.Shape)==source
base=dict(source='Polygon',name='Converted',split_index=6,sampled_tolerance_mm=1)
reject(**dict(base,source='Box'));reject(**dict(base,name='Box'))
reject(**dict(base,split_index=True));reject(**dict(base,sampled_tolerance_mm=1e-10))
reject(**dict(base,split_index=1));reject(**dict(base,sampled_tolerance_mm=2))
doc.Polygon_Section0.setExpression('Placement.Base.y','Box.Length');doc.recompute();reject(**base)
doc.Polygon_Section0.setExpression('Placement.Base.y',None);doc.Polygon_Section0.Placement.Base.y=0;doc.recompute()
doc.Polygon.Placement.Base.x=1;doc.recompute();reject(**base)
doc.Polygon.Placement.Base.x=0;doc.recompute()
bad=doc.addObject('Part::Feature','Invalid');doc.recompute();reject(**base);doc.removeObject(bad.Name);doc.recompute()
reserved=doc.addObject('App::FeaturePython','Converted_Section0');reject(**base);doc.removeObject(reserved.Name)
converted=worker.METHODS['convert_spline_loft'](**base)
assert converted['conversion']['source_point_edits_associative'] is False
assert all(s.GeometryCount==2 for s in doc.Converted.Sections)
reject(**dict(base,source='Converted',name='Again'))
App.closeDocument(doc.Name);worker._doc=None
source=ROOT/'out/tsung-vtol-reference/assembly.FCStd';source_hash=hashlib.sha256(source.read_bytes()).hexdigest()
worker.open_document(str(source));doc=worker.active();wing=doc.WingRight
assert len(wing.Sections)==11 and sum(s.GeometryCount for s in wing.Sections)==660
# The leading-edge branch division comes from the existing profile's minimum X.
world=[wing.Sections[0].Placement.multVec(e.StartPoint) for e in wing.Sections[0].Geometry]
split=min(range(len(world)),key=lambda i:world[i].x)
original=spline_conversion.geometry_hash(wing.Shape);original_volume=wing.Shape.Volume
result=worker.METHODS['convert_spline_loft'](source='WingRight',name='SmoothWingRight',split_index=split,sampled_tolerance_mm=.2)
smooth=doc.SmoothWingRight
assert spline_conversion.geometry_hash(wing.Shape)==original
assert result['conversion']['fit_evidence']['source_point_fit_max_mm']<1e-6
assert result['conversion']['fit_evidence']['sampled_curve_to_source_polyline_max_mm']<=.2
assert result['conversion']['source_point_count']==660 and len(result['conversion']['preserved_layout_expressions'])==11
assert all(s.GeometryCount==2 and s.FullyConstrained for s in smooth.Sections)
assert all(list(a.ExpressionEngine)==list(b.ExpressionEngine) for a,b in zip(wing.Sections,smooth.Sections))
worker.set_parameter('ReferenceDimensions','HalfSpan',610,summary=True)
assert abs(wing.Shape.BoundBox.YMax-610)<1e-5 and abs(smooth.Shape.BoundBox.YMax-610)<1e-5
worker.set_parameter('ReferenceDimensions','HalfSpan',600,summary=True)
# A source-coordinate edit does not change the converted snapshot.
snapshot=spline_conversion.geometry_hash(smooth.Shape)
points,_=spline_conversion._section(doc,wing.Sections[0]);changed=[p[:] for p in points];changed[3][0]+=.02
worker.set_profile(wing.Sections[0].Name,changed,summary=True)
assert spline_conversion.geometry_hash(smooth.Shape)==snapshot
worker.set_profile(wing.Sections[0].Name,points,summary=True)
assert abs(wing.Shape.Volume-original_volume)<1e-5
path=OUT/'converted.FCStd';doc.saveAs(str(path));App.closeDocument(doc.Name);worker._doc=None
worker.open_document(str(path));doc=worker.active()
assert json.loads(doc.SmoothWingRight.WrightSplineConversion)['source_geometry_sha256']==original
assert all(s.GeometryCount==2 for s in doc.SmoothWingRight.Sections)
worker.set_parameter('ReferenceDimensions','HalfSpan',605,summary=True)
assert abs(doc.SmoothWingRight.Shape.BoundBox.YMax-605)<1e-5
assert hashlib.sha256(source.read_bytes()).hexdigest()==source_hash
(OUT/'result.json').write_text(json.dumps(dict(passed=True,source_sha256=source_hash,source_file_unchanged=True,split_index=split,conversion=result,source_edges=660,spline_section_edges=22,layout_follows_after_reopen=True,source_coordinate_updates_associative=False,negative_controls_passed=True),indent=2))
print('Actual wing conversion passed: 660 polygon edges -> 22 spline edges, copied layout expressions, snapshot independence, reopen, rollback and source integrity')
App.closeDocument(doc.Name)
