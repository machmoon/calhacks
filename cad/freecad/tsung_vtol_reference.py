"""Evidence-labelled native silhouette reconstruction of Tsung Xu's June 2025 VTOL.
Published span/length/S3024 are constraints; all station/layout numbers are inferred.
Not original author CAD, complete internals, a manufacturing model or flight proof.
"""
import json,math,hashlib,os
from pathlib import Path
import FreeCAD as App
import Part
import native,worker,layout
from exchange import export_glb
ROOT=Path(__file__).resolve().parents[2];PROJECT=ROOT/'cad/projects/tsung_vtol'
OUT=Path(os.environ.get('WRIGHT_CAD_OUT',ROOT/'out/tsung-vtol-reference')).resolve();OUT.mkdir(parents=True,exist_ok=True)
worker.document('TsungVTOLReference');doc=worker.active()
layout.create(doc,'ReferenceDimensions',{name:dict(value=value,min=minimum,max=maximum) for name,value,minimum,maximum in [('HalfSpan',600,580,620),('BoomHalfSpacing',220,210,240),('TailTopHalfSpacing',175,150,190),('TailHeight',140,110,170)]})
params=doc.ReferenceDimensions
physical=[];envelopes=[];palette={}
def make(name,kind,p,xyz=(0,0,0),color='#d6d3c6'):
 native.make_part(doc,dict(name=name,kind=kind,params=p));o=doc.getObject(name);o.Placement.Base=App.Vector(*xyz);physical.append(name);palette[name]=color;return o
raw=[]
for line in (PROJECT/'references/s3024.dat').read_text().splitlines()[1:]:
 try:x,z=map(float,line.split());raw.append((x,z))
 except ValueError:continue
# UIUC trailing endpoints differ by 0.00001 chord; merge at their mean explicitly.
te=((raw[0][0]+raw[-1][0])/2,0);airfoil=[te]+raw[1:-1]
assert len(airfoil)>50
stations=[]
for eta in [0,.12,.25,.4,.55,.7,.82,.9,.95,.98,1]:
 chord=max(2,240*math.sqrt(max(0,1-eta*eta)))
 trailing=480-12*eta*eta
 stations.append(dict(eta=eta,y_mm=600*eta,chord_mm=chord,leading_x_mm=trailing-chord,trailing_x_mm=trailing))
for side,sgn in [('Right',1),('Left',-1)]:
 rings=[[[s['leading_x_mm']+x*s['chord_mm'],sgn*s['y_mm'],20+z*s['chord_mm']] for x,z in airfoil] for s in stations]
 wing=make('Wing'+side,'loft',dict(rings=rings),color='#e2dfd1')
 for section,s in zip(wing.Sections,stations):section.setExpression('Placement.Base.y',f'ReferenceDimensions.HalfSpan * {sgn*s["eta"]}')
 boom=make('Boom'+side,'tube',dict(radius=5,bore_radius=4,height=700),(40,sgn*220,-10),'#394349')
 boom.Placement.Rotation=App.Rotation(App.Vector(0,0,1),App.Vector(1,0,0));boom.setExpression('Placement.Base.y',f'ReferenceDimensions.BoomHalfSpacing * {sgn}')
 # Symmetric tail foil is a visual placeholder; actual tail airfoil is unknown.
 foil=[(1,0),(.75,.018),(.35,.03),(.08,.018),(0,0),(.08,-.018),(.35,-.03),(.75,-.018)]
 finrings=[]
 for x0,y,z in [(650,sgn*220,-10),(665,sgn*175,130)]:finrings.append([[x0+u*105,y+t*105,z] for u,t in foil])
 fin=make('Fin'+side,'loft',dict(rings=finrings),color='#b9bdba')
 fin.Sections[0].setExpression('Placement.Base.y',f'ReferenceDimensions.BoomHalfSpacing * {sgn}')
 fin.Sections[1].setExpression('Placement.Base.y',f'ReferenceDimensions.TailTopHalfSpacing * {sgn}')
 fin.Sections[1].setExpression('Placement.Base.z','ReferenceDimensions.TailHeight - 10 mm')
 for code,x in [('F',105),('R',585)]:
  name='LiftMotor'+code+('R' if sgn>0 else 'L')
  lift=make(name,'cylinder',dict(radius=17,height=22),(x,sgn*220,-5),'#4e5559');lift.setExpression('Placement.Base.y',f'ReferenceDimensions.BoomHalfSpacing * {sgn}')
  sweep=make('Sweep'+code+('R' if sgn>0 else 'L'),'cylinder',dict(radius=88.9,height=.6),(x,sgn*220,20),'#77a4a5')
  sweep.setExpression('Placement.Base.y',f'ReferenceDimensions.BoomHalfSpacing * {sgn}')
  physical.remove(sweep.Name);envelopes.append(sweep.Name)
tail=make('TopStabilizer','loft',dict(rings=[[[675+u*95,y,130+t*95] for u,t in foil] for y in [-175,175]]),color='#c5c6bf')
for section,sgn in zip(tail.Sections,[-1,1]):
 section.setExpression('Placement.Base.y',f'ReferenceDimensions.TailTopHalfSpacing * {sgn}')
 section.setExpression('Placement.Base.z','ReferenceDimensions.TailHeight - 10 mm')
pod_stations=[(0,2,2),(25,30,35),(100,65,55),(260,70,60),(360,50,45),(440,18,18),(470,15,15)]
outer=[];inner=[]
for i,(x,ry,rz) in enumerate(pod_stations):
 outer.append([[x,ry*math.cos(t*math.tau/32),-45+rz*math.sin(t*math.tau/32)] for t in range(32)])
 xi=x+1.2 if i==0 else x-1.2 if i==len(pod_stations)-1 else x
 inner.append([[xi,(ry-1.2)*math.cos(t*math.tau/32),-45+(rz-1.2)*math.sin(t*math.tau/32)] for t in range(32)])
make('PodClosedShell','loft_shell',dict(rings=outer,inner=inner));physical.remove('PodClosedShell')
# Provisional oval top service opening, observed qualitatively at video0:42.
opening=[[[172.5+87.5*math.cos(t*math.tau/32),42*math.sin(t*math.tau/32),z] for t in range(32)] for z in [-5,50]]
make('PodOpeningTool','loft',dict(rings=opening));physical.remove('PodOpeningTool')
pod=doc.addObject('Part::Cut','Pod');pod.Base=doc.PodClosedShell;pod.Tool=doc.PodOpeningTool;physical.append('Pod');palette['Pod']='#e1d7bd'
motor=make('PusherMotor','cylinder',dict(radius=17,height=22),(470,0,-45),'#4e5559');motor.Placement.Rotation=App.Rotation(App.Vector(0,0,1),App.Vector(1,0,0))
sweep=make('PusherSweep','cylinder',dict(radius=88.9,height=.6),(498,0,-45),'#77a4a5');sweep.Placement.Rotation=motor.Placement.Rotation;physical.remove(sweep.Name);envelopes.append(sweep.Name)
doc.recompute()
for name in physical+envelopes:
 evidence=native.describe(doc.getObject(name));assert evidence['solids']==1,name
bounds=Part.makeCompound([doc.getObject(n).Shape for n in physical]).BoundBox
assert abs(bounds.XLength-770)<1e-5 and abs(bounds.YLength-1200)<1e-5,(bounds.XLength,bounds.YLength)
print('Native geometry and1200x770 bounds valid; checking exterior envelopes',flush=True)
fit=worker.check(physical,require_connected=True)
print('Envelope checks complete; exporting checked native/STEP/GLB',flush=True)
exports=worker.save(str(OUT),physical)
export_glb([doc.getObject(n) for n in physical+envelopes],OUT/'reference-with-disks.glb')
# Lightweight render data from actual native shapes.
faces=[];colors=[]
for name in physical+envelopes:
 vertices,triangles=doc.getObject(name).Shape.tessellate(.4)
 for tri in triangles:faces.append([[vertices[j].x,vertices[j].y,vertices[j].z] for j in tri]);colors.append(palette[name])
(OUT/'preview-mesh.json').write_text(json.dumps(dict(faces=faces,colors=colors)))
report=dict(scope='Evidence-based approximate visual reconstruction; not original author CAD or fabricated internal assembly',published_constraints=dict(span_mm=1200,length_mm=770,main_airfoil='S3024'),axes='X nose to tail, Y span, Z up',physical_representation_names=physical,separate_swept_envelopes=envelopes,physical_representation_count=len(physical),bounds_mm=[[bounds.XMin,bounds.YMin,bounds.ZMin],[bounds.XMax,bounds.YMax,bounds.ZMax]],fit=fit,exports=exports,source_audit_sha256=hashlib.sha256((PROJECT/'reference-audit.json').read_bytes()).hexdigest(),inferred_parameters=dict(wing_stations=stations,wing_z_mm=20,boom=dict(start_x_mm=40,length_mm=700,half_spacing_mm=220,z_mm=-10,outer_radius_mm=5,inner_radius_mm=4),tail=dict(root_leading_x_mm=650,top_leading_x_mm=665,fin_chord_mm=105,root_half_spacing_mm=220,top_half_spacing_mm=175,root_z_mm=-10,top_z_mm=130,horizontal_leading_x_mm=675,horizontal_chord_mm=95,foil='symmetric polygon placeholder'),pod=dict(stations_x_radius_y_radius_z_mm=pod_stations,center_z_mm=-45,nominal_radial_reduction_mm=1.2,opening_center_xy_mm=[172.5,0],opening_radii_mm=[87.5,42]),motors=dict(radius_mm=17,height_mm=22,lift_x_mm=[105,585],lift_y_mm=[-220,220],pusher_start_x_mm=470)),swept_disc_radius_mm=88.9,disc_radius_basis='Nominal 7-inch diameter from author-reported 7042 prop identity; not blade geometry',trailing_edge_normalization=dict(source_endpoints=raw[0:1]+raw[-1:],merged=te,max_displacement_chord=0.000005),exact_reconstruction=False,internals_complete=False,manufacturing_complete=False,remaining=['Inference needs calibrated multi-view comparison','Wing and tail solids are exterior envelopes, not manufactured skins','No claimed internal hardware placements','Unresolved envelope overlaps and disconnected mounting interfaces','No original CAD release found'])
(OUT/'result.json').write_text(json.dumps(report,indent=2))
App.closeDocument(doc.Name)
# Reopening must preserve the native section expressions, not just a frozen shape.
saved=OUT/'assembly.FCStd';saved_hash=hashlib.sha256(saved.read_bytes()).hexdigest();doc=App.openDocument(str(saved))
native.set_parameter(doc,'ReferenceDimensions','HalfSpan',610);doc.recompute()
assert abs(doc.WingRight.Shape.BoundBox.YMax-610)<1e-6
assert abs(doc.WingLeft.Shape.BoundBox.YMin+610)<1e-6
report['saved_reopen_span_edit_passed']=True
report['typed_api_span_edit_passed']=True
native.set_parameter(doc,'ReferenceDimensions','BoomHalfSpacing',230)
assert abs(doc.LiftMotorFR.Shape.CenterOfMass.y-230)<1e-6 and abs(doc.SweepFR.Shape.CenterOfMass.y-230)<1e-6
assert abs(doc.LiftMotorFL.Shape.CenterOfMass.y+230)<1e-6
report['typed_api_boom_layout_edit_passed']=True
App.closeDocument(doc.Name);assert hashlib.sha256(saved.read_bytes()).hexdigest()==saved_hash
(OUT/'result.json').write_text(json.dumps(report,indent=2))
print(json.dumps(dict(parts=len(physical),discs=len(envelopes),bounds=report['bounds_mm'],clashes=fit['overlap_count'],reopen_edit=True)))
