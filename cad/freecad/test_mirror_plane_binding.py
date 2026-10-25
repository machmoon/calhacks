"""Typed native mirror-plane bindings, wrong-type rejection and saved aircraft edit."""
import hashlib,json,tempfile
from pathlib import Path
import FreeCAD as App
import native,worker
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/freecad-mirror-plane-binding';OUT.mkdir(parents=True,exist_ok=True)
call=worker.METHODS;worker.document('MirrorPlaneBinding');doc=worker.active()
call['make_parameters'](name='RotorLayout',parameters={'RearX':dict(value=561.18,min=500,max=585),'PlaneOffset':dict(value=30,min=10,max=50)})
call['make_part'](spec=dict(name='FrontRight',kind='cylinder',params=dict(radius=3,height=6)))
call['place'](object_name='FrontRight',position=[105,220,6])
call['mirror'](name='RearRight',source='FrontRight',normal=[1,0,0],origin=[345,0,0])
call['mirror'](name='RearLeft',source='RearRight',normal=[0,1,0])
call['bind_parameter'](object_name='RearRight',parameter='Base.x',source_object='RotorLayout',source_parameter='RearX',scale=.5,offset_mm=52.5)
assert abs(doc.RearRight.Base.x-333.09)<1e-7
for name,sign in [('RearRight',1),('RearLeft',-1)]:
 center=doc.getObject(name).Shape.Solids[0].CenterOfMass;assert abs(center.x-561.18)<1e-7 and abs(center.y-sign*220)<1e-7
# All three vector coordinates are supported, with actual normal-dependent motion.
for axis,normal,source_center in [('y',[0,1,0],220),('z',[0,0,1],9)]:
 name='Plane'+axis.upper();call['mirror'](name=name,source='FrontRight',normal=normal)
 call['bind_parameter'](object_name=name,parameter='Base.'+axis,source_object='RotorLayout',source_parameter='PlaneOffset')
 assert abs(getattr(doc.getObject(name).Shape.Solids[0].CenterOfMass,axis)-(60-source_center))<1e-7
call['make_part'](spec=dict(name='WrongBox',kind='box',params=dict(x=2,y=2,z=2)))
rejected=[]
for obj,param in [('FrontRight','Base.x'),('WrongBox','Base.x'),('WrongBox','Base.y'),('WrongBox','Base.z'),('RearRight','Base'),('RearRight','Base.foo'),('RearRight','Base.x.y'),('RearRight','Normal.x')]:
 before=list(doc.getObject(obj).ExpressionEngine)
 try:call['bind_parameter'](object_name=obj,parameter=param,source_object='RotorLayout',source_parameter='RearX')
 except ValueError:pass
 else:raise AssertionError('unsupported target accepted: '+obj+'.'+param)
 assert before==list(doc.getObject(obj).ExpressionEngine);rejected.append(obj+'.'+param)
controls=native.parameter_bindings(doc.RearLeft);assert any(c['object_name']=='RotorLayout' and c['parameter']=='RearX' for c in controls)
with tempfile.TemporaryDirectory(prefix='wright-mirror-plane-') as tmp:
 path=Path(tmp)/'assembly.FCStd';doc.saveAs(str(path));App.closeDocument(doc.Name);worker._doc=None;worker.restore(str(path));doc=worker.active()
 call['set_parameter'](object_name='RotorLayout',parameter='RearX',value=560.18,summary=True)
 assert abs(doc.RearRight.Base.x-332.59)<1e-7
 for name in ['RearRight','RearLeft']:assert abs(doc.getObject(name).Shape.Solids[0].CenterOfMass.x-560.18)<1e-7
 call['set_parameter'](object_name='RotorLayout',parameter='PlaneOffset',value=31,summary=True)
 assert abs(doc.PlaneY.Shape.Solids[0].CenterOfMass.y-(62-220))<1e-7 and abs(doc.PlaneZ.Shape.Solids[0].CenterOfMass.z-(62-9))<1e-7
App.closeDocument(doc.Name);worker._doc=None
# Exercise the exact real saved aircraft through the public typed action mapping.
source=ROOT/'out/tsung-vtol-rear-rotor-layout/assembly.FCStd';digest=hashlib.sha256(source.read_bytes()).hexdigest();worker.open_document(str(source));doc=worker.active();original_x=float(doc.RearRotorLayout.RearMotorX)
call['bind_parameter'](object_name='LiftRRMotor',parameter='Base.x',source_object='RearRotorLayout',source_parameter='RearMotorX',scale=.5,offset_mm=52.5)
call['set_parameter'](object_name='RearRotorLayout',parameter='RearMotorX',value=original_x-1,summary=True)
assert abs(doc.LiftRRMotor.Base.x-(105+original_x-1)/2)<1e-7
for name,sign in [('LiftRRMotor',1),('LiftRLMotor',-1)]:
 center=doc.getObject(name).Shape.Solids[0].CenterOfMass;assert abs(center.x-(original_x-1))<1e-7 and abs(center.y-sign*220)<1e-7
assert hashlib.sha256(source.read_bytes()).hexdigest()==digest
(OUT/'result.json').write_text(json.dumps(dict(passed=True,axes=['x','y','z'],wrong_targets_rejected=rejected,reopened_fixture_edit=True,reopened_aircraft_edit=True,source_preserved=True,source_sha256=digest,formula='mirror plane X = 0.5 * rear motor X + 52.5 mm'),indent=2));print('Typed mirror-plane bindings passed',flush=True)
