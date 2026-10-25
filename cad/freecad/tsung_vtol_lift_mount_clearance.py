"""Measure nominal motor-fastener clearance, not manufacturer allowable depth."""
import hashlib,json
from pathlib import Path
import FreeCAD as App
ROOT=Path(__file__).resolve().parents[2];out=ROOT/'out/tsung-vtol-lift-mount';p=out/'assembly.FCStd';digest=hashlib.sha256(p.read_bytes()).hexdigest();r=json.loads((out/'checks-before-export.json').read_text());doc=App.openDocument(str(p));upper=doc.getObject(r['instances']['FR']['Upper']).Shape;motor=doc.getObject(r['instances']['FR']['Motor']).Shape
checks=[]
for i in range(4):
 shank=doc.getObject(f'LiftMotorScrew{i}Shaft').Shape;tool=doc.getObject(f'ProvisionalMotorThread{i}').Shape
 clearance=shank.distToShape(upper)[0];tip=tool.BoundBox.ZMax-shank.BoundBox.ZMax;engagement=shank.BoundBox.ZMax-doc.AOS2807BodyEnvelope.Shape.BoundBox.ZMin
 assert abs(clearance-.2)<1e-6 and abs(tip-.5)<1e-6 and abs(engagement-2)<1e-6
 moved=shank.copy();moved.translate(App.Vector(0,0,.6));v=moved.common(motor).Volume;assert v>1e-4
 checks.append(dict(index=i,radial_plate_clearance_mm=clearance,provisional_thread_engagement_mm=engagement,provisional_blind_floor_tip_clearance_mm=tip,shank_up_0_6_floor_interference_mm3=v))
assert hashlib.sha256(p.read_bytes()).hexdigest()==digest
(out/'fastener-clearance.json').write_text(json.dumps(dict(scope='Nominal proposed geometry only; allowable manufacturer screw depth unknown',master_checks=checks,other_instances='Native exact mirrors of checked master',native_preserved=True),indent=2))
