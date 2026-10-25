"""Native dependent rear-rotor layout correction; explicitly proposed clearance."""
import base64,hashlib,json,math
from pathlib import Path
import FreeCAD as App
import native,worker,layout
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/tsung-vtol-rear-rotor-layout';SOURCE=ROOT/'out/tsung-vtol-lift-mount/assembly.FCStd'
search=json.loads((OUT/'shift-search.json').read_text());previous=json.loads((SOURCE.parent/'result.json').read_text());digest=hashlib.sha256(SOURCE.read_bytes()).hexdigest();assert digest==search['source_sha256']
worker.open_document(str(SOURCE));doc=worker.active();physical=previous['physical'];contacts=previous['contacts'];x=search['selected_rear_motor_x_mm']
unchanged_names=[n for n in physical if n not in list(previous['instances']['RR'].values())+list(previous['instances']['RL'].values())]
original={n:doc.getObject(n).Shape.copy() for n in unchanged_names}
worker.METHODS['make_parameters'](name='RearRotorLayout',parameters=dict(RearMotorX=dict(value=x,min=500,max=585)))
bindings=[]
for name in previous['instances']['RR'].values():
 o=doc.getObject(name);assert o.TypeId=='Part::Mirroring' and abs(o.Base.x-345)<1e-6
 binding=worker.METHODS['bind_parameter'](object_name=name,parameter='Base.x',source_object='RearRotorLayout',source_parameter='RearMotorX',scale=.5,offset_mm=52.5);bindings.append(binding)
for code in ['RR','RL']:
 binding=worker.METHODS['bind_parameter'](object_name='Sweep'+code,parameter='Placement.Base.x',source_object='RearRotorLayout',source_parameter='RearMotorX');bindings.append(binding)
doc.recompute()
def sweep_checks():
 pairs=[];names=['SweepFR','SweepFL','SweepRR','SweepRL','PusherSweep']
 own={('Sweep'+c):parts['Motor'] for c,parts in previous['instances'].items()};own['PusherSweep']='PusherMotor'
 for sweep in names:
  a=doc.getObject(sweep).Shape
  for name in physical:
   if name==own[sweep]:continue
   b=doc.getObject(name).Shape;pairs.append(dict(sweep=sweep,part=name,distance_mm=a.distToShape(b)[0],overlap_mm3=a.common(b).Volume))
 between=[]
 for i,an in enumerate(names):
  for bn in names[i+1:]:
   a=doc.getObject(an).Shape;b=doc.getObject(bn).Shape;between.append(dict(a=an,b=bn,distance_mm=a.distToShape(b)[0],overlap_mm3=a.common(b).Volume))
 body_clearance=[]
 for code in previous['instances']:
  matrix=App.Matrix()
  if code.startswith('R'):matrix.A11=-1;matrix.A14=105+float(doc.RearRotorLayout.RearMotorX)
  if code.endswith('L'):matrix.A22=-1
  body=doc.AOS2807BodyEnvelope.Shape.transformGeometry(matrix);sweep=doc.getObject('Sweep'+code).Shape
  gap=body.distToShape(sweep)[0];assert gap>0
  body_clearance.append(dict(instance=code,body_to_own_sweep_mm=gap))
 return dict(sweep_to_physical=pairs,rotor_to_rotor=between,own_motor_exclusion=own,lift_body_to_own_sweep=body_clearance)
def fin_gaps():return {c:doc.getObject('Sweep'+c).Shape.distToShape(doc.getObject('FinTopRetention'+side).Shape)[0] for c,side in [('RR','Right'),('RL','Left')]}
def access():
 rows=[]
 for code in previous['instances']:
  matrix=App.Matrix()
  if code.startswith('R'):matrix.A11=-1;matrix.A14=105+float(doc.RearRotorLayout.RearMotorX)
  if code.endswith('L'):matrix.A22=-1
  for key in [f'LiftMotorKey{i}' for i in range(4)]+[f'LiftClampKey{i}' for i in range(2)]:
   shape=doc.getObject(key).Shape.transformGeometry(matrix)
   for name in physical:
    target=doc.getObject(name).Shape;a=shape.BoundBox;b=target.BoundBox
    separated=any(getattr(a,t+'Max')<getattr(b,t+'Min') or getattr(b,t+'Max')<getattr(a,t+'Min') for t in ['X','Y','Z'])
    volume=0 if separated else shape.common(target).Volume;assert volume<1e-6,(code,key,name,volume)
    rows.append(dict(instance=code,tool=key,part=name,overlap_mm3=volume))
 return rows
print('Rear mirrors/sweeps linked; checking physical assembly',flush=True)
fit=worker.check(physical,contacts,tolerance_mm=.001,require_connected=True);assert fit['overlap_count']==0 and fit['unmet_contact_count']==0
print('Physical fit checked; checking all five sweeps',flush=True)
sweeps=sweep_checks();gaps=fin_gaps();assert min(gaps.values())>=5
sweep_failures=[p for p in sweeps['sweep_to_physical']+sweeps['rotor_to_rotor'] if p['overlap_mm3']>1e-6 or p['distance_mm']<1e-6]
print('Sweeps recorded; checking tool access and dependencies',flush=True)
keys=access();local=[n for n in physical if n not in ['WingRight','WingLeft','Pod','PusherMotor']]
native.set_parameter(doc,'ReferenceDimensions','BoomHalfSpacing',230)
spacing_fit=worker.check(local,contacts,tolerance_mm=.001,require_connected=True);assert spacing_fit['passed'];spacing_gaps=fin_gaps();assert min(spacing_gaps.values())>=5
spacing_sweeps=sweep_checks();spacing_access=access()
native.set_parameter(doc,'RearRotorLayout','RearMotorX',x-1)
for code in ['RR','RL']:assert abs(doc.getObject(previous['instances'][code]['Motor']).Shape.Solids[0].CenterOfMass.x-(x-1))<1e-6
x_edit_fit=worker.check(local,contacts,tolerance_mm=.001,require_connected=True);assert x_edit_fit['passed']
x_edit_sweeps=sweep_checks()
native.set_parameter(doc,'RearRotorLayout','RearMotorX',x);native.set_parameter(doc,'ReferenceDimensions','BoomHalfSpacing',220)
unchanged=[]
for name,a in original.items():
 b=doc.getObject(name).Shape
 if a.exportBrepToString()==b.exportBrepToString():diff=[0,0];method='identical_brep'
 else:diff=[a.cut(b).Volume,b.cut(a).Volume];method='bidirectional_difference'
 assert max(diff)<1e-5,(name,diff);unchanged.append(dict(object=name,method=method,difference_mm3=diff))
fit=worker.check(physical,contacts,tolerance_mm=.001,require_connected=True)
report=dict(scope='Proposed rear-rotor layout correction; target5mm fin clearance is provisional',source_sha256=digest,shift_search=search,physical=physical,instances=previous['instances'],contacts=contacts,bindings=bindings,fit=fit,sweeps=sweeps,sweep_failures=sweep_failures,fin_clearance_mm=gaps,spacing_230_fin_clearance_mm=spacing_gaps,spacing_edit_fit=spacing_fit,spacing_230_sweeps=spacing_sweeps,spacing_230_tool_access=spacing_access,rear_x_minus1_edit_fit=x_edit_fit,rear_x_minus1_sweeps=x_edit_sweeps,tool_access=keys,unchanged_physical=unchanged,limitations=previous['limitations']+['5mm target is a provisional geometric envelope margin, not dynamic blade deflection allowance','Only rear rotor/mount longitudinal placement changes; primary motor and prop dimensions and fin shapes remain unchanged','Full aircraft retains three disconnected physical groups'],unresolved_motor_depth=previous['missing_manufacturer_data'])
(OUT/'checks-before-export.json').write_text(json.dumps(report,indent=2));print('Checks complete; exporting',flush=True)
report['exports']=worker.save(str(OUT),physical)
try:
 image=worker.render(physical,deflection_mm=10,max_triangles=100000);(OUT/'aircraft.png').write_bytes(base64.b64decode(image.pop('content_blocks')[0]['source']['data']));(OUT/'render.json').write_text(json.dumps(image,indent=2))
except ValueError as error:report['render_failure']=str(error)
(OUT/'result.json').write_text(json.dumps(report,indent=2))
saved=Path(report['exports']['native']);saved_hash=hashlib.sha256(saved.read_bytes()).hexdigest();App.closeDocument(doc.Name);worker._doc=None;worker.restore(str(saved));doc=worker.active()
native.set_parameter(doc,'ReferenceDimensions','BoomHalfSpacing',230);native.set_parameter(doc,'RearRotorLayout','RearMotorX',x-1)
reopened=worker.check(local,contacts,tolerance_mm=.001,require_connected=True);assert reopened['passed']
for code in ['RR','RL']:assert abs(doc.getObject(previous['instances'][code]['Motor']).Shape.Solids[0].CenterOfMass.x-(x-1))<1e-6
assert hashlib.sha256(saved.read_bytes()).hexdigest()==saved_hash and hashlib.sha256(SOURCE.read_bytes()).hexdigest()==digest
report.update(saved_reopen_parameter_edits_passed=True,source_preserved=True);(OUT/'result.json').write_text(json.dumps(report,indent=2));print('COMPLETE',flush=True)
