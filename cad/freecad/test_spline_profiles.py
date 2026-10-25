"""Actual typed API coverage for editable two-curve S3024 spline lofts."""
import hashlib,json,os
from pathlib import Path
import FreeCAD as App
import native,worker,profiles,Part

ROOT=Path(__file__).resolve().parents[2]
OUT=Path(os.environ.get('WRIGHT_CAD_OUT',ROOT/'out/freecad-spline-profiles')).resolve()
OUT.mkdir(parents=True,exist_ok=True)
source=ROOT/'cad/projects/tsung_vtol/references/s3024.dat'
raw=[]
for line in source.read_text().splitlines()[1:]:
    try:raw.append(tuple(map(float,line.split())))
    except ValueError:pass
te=((raw[0][0]+raw[-1][0])/2,0)
foil=[te]+raw[1:-1]
split=min(range(len(foil)),key=lambda i:foil[i][0])
def ring(chord,y):return [[chord*x,y,chord*z] for x,z in foil]
rings=[ring(240,0),ring(120,180)]
# Independent native-distance comparison for the analytical evidence primitive.
vertices=[App.Vector(0,0,0),App.Vector(10,0,0),App.Vector(10,5,0),App.Vector(0,5,0)]
queries=[App.Vector(5,2,3),App.Vector(-1,-2,0),App.Vector(10,3,0),App.Vector(5,6,0)]
poly=Part.makePolygon(vertices+vertices[:1])
analytical=profiles._polyline_distances(queries,vertices)
assert all(abs(distance-Part.Vertex(point).distToShape(poly)[0])<1e-9 for distance,point in zip(analytical,queries))
worker.document('SplineAPIRegression');doc=worker.active()
worker.METHODS['make_parameters'](name='SplineDims',parameters={'Span':{'value':180,'min':100,'max':250}})
spec={'name':'SmoothFoil','kind':'spline_loft','params':{'rings':rings,'split_index':split,'sampled_tolerance_mm':.2}}
worker.METHODS['make_part'](spec=spec)
loft=doc.SmoothFoil
assert len(loft.Shape.Faces)==4
assert all(s.GeometryCount==2 and s.FullyConstrained and s.WrightSplineProfile for s in loft.Sections)
evidence=[json.loads(s.WrightSplineEvidence) for s in loft.Sections]
assert max(e['source_point_fit_max_mm'] for e in evidence)<1e-6
assert max(e['sampled_curve_to_source_polyline_max_mm'] for e in evidence)<.2
assert all(e['certified_hausdorff_bound'] is False for e in evidence)
assert all('native curve samples' in e['method'] for e in evidence)
checks=['analytical_polyline_distance_matches_native','typed_creation_four_faces','source_coordinate_fit_and_sampled_deviation','fixed_native_two_curve_sections']

def rejected(label,fn):
    before=worker.geometry_fingerprint([loft])
    names=[o.Name for o in doc.Objects]
    provenance=[s.WrightSplineSourceLocalPoints for s in loft.Sections]
    try:fn()
    except (ValueError,TypeError):pass
    else:raise AssertionError('accepted '+label)
    assert worker.geometry_fingerprint([loft])==before
    assert [o.Name for o in doc.Objects]==names
    assert [s.WrightSplineSourceLocalPoints for s in loft.Sections]==provenance
    checks.append(label)

for label,change in [('nonfinite_tolerance',{'sampled_tolerance_mm':float('nan')}),
    ('bool_split',{'split_index':True}),('too_many_sections',{'rings':rings*9}),
    ('sampled_fit_gate',{'sampled_tolerance_mm':.001})]:
    params={**spec['params'],**change}
    rejected(label,lambda params=params:worker.METHODS['make_part'](spec={'name':'Rejected','kind':'spline_loft','params':params}))
rejected('duplicate_name',lambda:worker.METHODS['make_part'](spec=spec))
huge=[[p[:] for p in r] for r in rings];huge[0][0][0]=1e7
rejected('bounded_coordinates',lambda:worker.METHODS['make_part'](spec={'name':'Huge','kind':'spline_loft','params':{**spec['params'],'rings':huge}}))
section=loft.Sections[1]
worker.METHODS['bind_parameter'](object_name=section.Name,parameter='Placement.Base.y',source_object='SplineDims',source_parameter='Span')
expressions=list(section.ExpressionEngine)
worker.METHODS['set_profile'](object_name=section.Name,points=ring(110,180))
assert section.GeometryCount==2 and list(section.ExpressionEngine)==expressions
checks.append('typed_section_edit_preserves_spline_and_span_expression')
before_volume=loft.Shape.Volume
worker.METHODS['set_parameter'](object_name='SplineDims',parameter='Span',value=200)
assert abs(native.shape_bounds(loft.Shape).YMax-200)<1e-6
assert abs(loft.Shape.Volume-before_volume*200/180)<1e-5
checks.append('shared_span_expression_recomputes_volume')
bad=ring(110,200);bad[2]=bad[1]
rejected('duplicate_point_edit_rollback',lambda:worker.METHODS['set_profile'](object_name=section.Name,points=bad))
bad_plane=ring(110,200);bad_plane[3][1]+=1
rejected('nonplanar_edit_rollback',lambda:worker.METHODS['set_profile'](object_name=section.Name,points=bad_plane))
rejected('source_count_change_rollback',lambda:worker.METHODS['set_profile'](object_name=section.Name,points=ring(110,200)[:-1]))
worker.check(['SmoothFoil']);exports=worker.save(str(OUT),['SmoothFoil'])
source_points=section.WrightSplineSourceLocalPoints
stored_evidence=section.WrightSplineEvidence
App.closeDocument(doc.Name);worker._doc=None
worker.restore(exports['native']);doc=worker.active();loft=doc.SmoothFoil;section=loft.Sections[1]
assert section.WrightSplineSourceLocalPoints==source_points
assert section.WrightSplineEvidence==stored_evidence
assert list(section.ExpressionEngine)==expressions
worker.METHODS['set_parameter'](object_name='SplineDims',parameter='Span',value=220)
assert abs(native.shape_bounds(loft.Shape).YMax-220)<1e-6
worker.METHODS['set_profile'](object_name=section.Name,points=ring(105,220))
assert section.GeometryCount==2 and section.FullyConstrained
assert json.loads(section.WrightSplineEvidence)['source_unique_points']==len(foil)
checks.append('reopen_provenance_native_span_and_typed_section_edit')
(OUT/'result.json').write_text(json.dumps(dict(passed=True,checks=checks,
    source=str(source),source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
    initial_section_evidence=evidence,faces=4,exports=exports,
    exact_polygon_equivalence=False,scope='Typed smooth profile representation regression; not a wing assembly or certified continuous approximation bound'),indent=2))
App.closeDocument(doc.Name);worker._doc=None
print('Spline profile typed API regression passed:',len(checks),'checks',flush=True)
