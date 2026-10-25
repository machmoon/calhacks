"""Typed optional spline-loft modes: strict input, native geometry and reopen."""
import json
from pathlib import Path
import FreeCAD as App
import native,worker
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/freecad-spline-modes';OUT.mkdir(parents=True,exist_ok=True)
raw=[]
for line in (ROOT/'cad/projects/tsung_vtol/references/s3024.dat').read_text().splitlines()[1:]:
 try:raw.append(tuple(map(float,line.split())))
 except ValueError:pass
foil=[((raw[0][0]+raw[-1][0])/2,0)]+raw[1:-1]
split=min(range(len(foil)),key=lambda i:foil[i][0])
def ring(chord,y,x0):return [[x0+chord*x,y,chord*z] for x,z in foil]
def params(x0):return dict(rings=[ring(240,0,x0),ring(200,80,x0),ring(130,200,x0)],split_index=split,sampled_tolerance_mm=.2)
worker.METHODS['new_document'](name='SplineModes') if 'new_document' in worker.METHODS else worker.document('SplineModes')
doc=worker.active()
worker.METHODS['make_part'](spec=dict(name='DefaultRuled',kind='spline_loft',params=params(0)))
worker.METHODS['make_part'](spec=dict(name='ExplicitRuled',kind='spline_loft',params={**params(400),'ruled':True}))
worker.METHODS['make_part'](spec=dict(name='Smooth',kind='spline_loft',params={**params(800),'ruled':False}))
a,b,c=doc.DefaultRuled,doc.ExplicitRuled,doc.Smooth
assert a.Ruled and b.Ruled and not c.Ruled
assert [len(o.Shape.Faces) for o in (a,b,c)]==[6,6,4]
assert abs(a.Shape.Volume-b.Shape.Volume)<1e-5
assert abs(a.Shape.Volume-c.Shape.Volume)>1
assert all(o.Shape.isValid() and len(o.Shape.Solids)==1 for o in (a,b,c))
for obj,expected in [(a,True),(b,True),(c,False)]:
 meta=json.loads(obj.WrightSplineRepresentation);assert meta['ruled'] is expected
 assert meta['loft_mode']==('ruled' if expected else 'smooth')
checks=['omitted_flag_preserves_ruled_default','explicit_true_matches_default','false_creates_native_four_face_smooth_loft','saved_provenance_matches_native_mode']
for i,value in enumerate(['false',0,1,None,[],{}]):
 before=worker.geometry_fingerprint([a,b,c]);names=[o.Name for o in doc.Objects]
 try:worker.METHODS['make_part'](spec=dict(name='Rejected'+str(i),kind='spline_loft',params={**params(1200),'ruled':value}))
 except ValueError as error:assert 'boolean' in str(error)
 else:raise AssertionError('Non-boolean ruled accepted: '+repr(value))
 assert before==worker.geometry_fingerprint([a,b,c]) and names==[o.Name for o in doc.Objects]
 checks.append('strict_bool_reject_'+type(value).__name__+'_'+str(i))
worker.METHODS['make_parameters'](name='Dimensions',parameters={'TipY':dict(value=200,min=200,max=230)})
section=c.Sections[-1]
worker.METHODS['bind_parameter'](object_name=section.Name,parameter='Placement.Base.y',source_object='Dimensions',source_parameter='TipY')
expr=list(section.ExpressionEngine)
worker.METHODS['set_profile'](object_name=section.Name,points=ring(135,200,800))
assert not c.Ruled and len(c.Shape.Faces)==4 and list(section.ExpressionEngine)==expr
assert json.loads(c.WrightSplineRepresentation)['ruled'] is False
checks.append('typed_profile_edit_retains_smooth_mode')
checkpoint=worker.checkpoint(str(OUT));source_data=[s.WrightSplineSourceLocalPoints for s in c.Sections]
App.closeDocument(doc.Name);worker._doc=None;worker.restore(checkpoint['checkpoint']);doc=worker.active();c=doc.Smooth
assert not c.Ruled and len(c.Shape.Faces)==4
assert [s.WrightSplineSourceLocalPoints for s in c.Sections]==source_data
assert json.loads(c.WrightSplineRepresentation)['loft_mode']=='smooth'
worker.METHODS['set_parameter'](object_name='Dimensions',parameter='TipY',value=220)
assert not c.Ruled and abs(native.shape_bounds(c.Shape).YMax-220)<1e-6 and c.Shape.isValid()
checks.extend(['saved_reopen_retains_native_mode_and_provenance','typed_dependency_edit_after_reopen'])
report=dict(passed=True,checks=checks,checkpoint=checkpoint,default_ruled=True,smooth_native_faces=len(c.Shape.Faces),
    creation_faces=dict(default=6,explicit_true=6,smooth_false=4),scope='Actual typed native loft mode regression; not an assembly or automatic wall-thickness guarantee')
(OUT/'result.json').write_text(json.dumps(report,indent=2));print('PASS',len(checks),'checks',flush=True)
