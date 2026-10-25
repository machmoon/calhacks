"""Separate proposed aircraft with genuinely non-ruled spline wing surfaces.

Preserves supplied section sketches, real skin/cavity/rib/spar dependencies, and
all other physical parts. No original-author internals or flight approval claim.
"""
import hashlib,json,time,os,sys,math
from pathlib import Path
import FreeCAD as App
import Part
import native,worker,surface_preview
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/os.environ.get('WRIGHT_SMOOTH_OUT','out/tsung-vtol-smooth-aircraft');OUT.mkdir(parents=True,exist_ok=True)
AIRCRAFT=ROOT/os.environ.get('WRIGHT_SMOOTH_AIRCRAFT_SOURCE','out/tsung-vtol-lift-mount/assembly.FCStd')
WING=ROOT/os.environ.get('WRIGHT_SMOOTH_WING_SOURCE','out/tsung-vtol-servo-fit/assembly.FCStd')
manifest=AIRCRAFT.parent/('result.json' if (AIRCRAFT.parent/'result.json').exists() else 'checks-before-export.json')
prior=json.loads(manifest.read_text())
source_hashes={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in [AIRCRAFT,WING]}
started=time.time()
report=dict(scope='Proposed full aircraft with non-ruled spline wing; independent candidate, not original-author CAD',
    status='building',source_sha256=source_hashes,inherited_aircraft_evidence=dict(path=str(manifest),sha256=hashlib.sha256(manifest.read_bytes()).hexdigest()),manufacturing_complete=False,flight_validated=False,
    limitations=['All hidden wing structure is provisional engineering, not authored reference internals',
    'Non-ruled surfaces interpolate the same sections but change geometry between stations',
    'Default OD4/ID3 spar remains structurally unsized',
    'Volume integrals of curved solids are not a trusted mass estimate',
    'Original wing/boom/pod disconnections remain unless explicitly resolved by later hardware',
    'Static servo envelope omits horn, pushrod, bracket, hatch and cable access'])
def record_failure(kind,error,traceback):
 report['status']='failed';report['failure']=dict(type=kind.__name__,message=str(error))
 (OUT/'failure.json').write_text(json.dumps(report,indent=2))
 sys.__excepthook__(kind,error,traceback)
sys.excepthook=record_failure
def phase(name):
 report['status']=name;report['elapsed_seconds']=time.time()-started
 (OUT/'progress.json').write_text(json.dumps(report,indent=2));print(name,flush=True)
if os.environ.get('WRIGHT_SMOOTH_RESUME')=='1':
 report=json.loads((OUT/'progress.json').read_text())
 report.setdefault('render_failures',[]).append('Initial1000x750 preview exceeded300M projected-pixel budget; native candidate unchanged')
 source=App.openDocument(str(AIRCRAFT))
 original={n:hashlib.sha256(source.getObject(n).Shape.exportBrepToString().encode()).hexdigest() for n in prior['physical'] if n not in ['WingRight','WingLeft']}
 App.closeDocument(source.Name)
 worker.open_document(str(OUT/'proposed-unverified.FCStd'));doc=worker.active()
 physical=report['physical'];contacts=report['contacts'];servo=doc.MG90SStaticEnvelope
else:
 phase('Loading preserved source documents')
 wing=App.openDocument(str(WING))
 wing_physical=['RightSkin','RightSpar']+[f'RightRib{i}' for i in range(6)]
 wing_physical+= [n.replace('Right','Left') for n in wing_physical]
 # Native FreeCAD non-ruled loft interpolation, not mesh subdivision or normal tricks.
 for name in ['RightOuterContour','RightCavity']:
  obj=wing.getObject(name);obj.Ruled=(name=='RightCavity' and os.environ.get('WRIGHT_SMOOTH_CAVITY_RULED')=='1')
  if 'WrightSplineRepresentation' in obj.PropertiesList:
   data=json.loads(obj.WrightSplineRepresentation);data['loft_mode']='native ruled cavity interpolation' if obj.Ruled else 'native non-ruled spline interpolation';data['exact_ruled_baseline_equivalence']=False;obj.WrightSplineRepresentation=json.dumps(data)
 phase('Recomputing non-ruled exterior, selected cavity construction and dependent ribs')
 wing.recompute()
 for name in wing_physical+['RightOuterContour','RightCavity']:
  shape=wing.getObject(name).Shape
  if shape.isNull() or not shape.isValid() or len(shape.Solids)!=1:raise ValueError('Invalid non-ruled candidate: '+name)
 report['native_wing_face_counts']={n:len(wing.getObject(n).Shape.Faces) for n in wing_physical+['RightOuterContour','RightCavity']}
 report['native_loft_ruled']={n:bool(wing.getObject(n).Ruled) for n in ['RightOuterContour','RightCavity']}
 report['source_section_counts']={n:len(wing.getObject(n).Sections) for n in ['RightOuterContour','RightCavity']}
 continuity=[]
 for i,face in enumerate(wing.RightOuterContour.Shape.Faces):
  surface=face.Surface;row=dict(face=i,surface_type=type(surface).__name__)
  if hasattr(surface,'getUMultiplicities'):
   u=list(surface.getUMultiplicities());v=list(surface.getVMultiplicities())
   row.update(u_degree=surface.UDegree,v_degree=surface.VDegree,u_multiplicities=u,v_multiplicities=v,
     minimum_interior_u_continuity=surface.UDegree-max(u[1:-1],default=0),minimum_interior_v_continuity=surface.VDegree-max(v[1:-1],default=0))
  continuity.append(row)
 report['native_outer_surface_continuity']=continuity
 report['continuity_scope']='Within each aerodynamic native surface; continuity across leading/trailing edges and the bilateral root seam is not certified'

 wing.saveAs(str(OUT/'wing-candidate.FCStd'))
 phase('Copying native wing dependency graph into full aircraft')
 worker.open_document(str(AIRCRAFT));doc=worker.active()
 original={n:hashlib.sha256(doc.getObject(n).Shape.exportBrepToString().encode()).hexdigest() for n in prior['physical'] if n not in ['WingRight','WingLeft']}
 copied=doc.copyObject([wing.getObject(n) for n in wing_physical+['MG90SStaticEnvelope']],True)
 assert [o.Name for o in copied]==wing_physical+['MG90SStaticEnvelope']
 # Replaced exterior bodies must not survive as duplicate physical outputs in
 # the new document. The original aircraft file remains unchanged on disk.
 obsolete_sections=[]
 for old_name in ['WingRight','WingLeft']:
  old=doc.getObject(old_name)
  if old is not None:
   if old.InList:raise ValueError('Original wing has consumers requiring explicit migration: '+old_name)
   obsolete_sections.extend(list(getattr(old,'Sections',[])));doc.removeObject(old_name)
 for section in obsolete_sections:
  if not section.InList:doc.removeObject(section.Name)
 report['replaced_original_wing_bodies_removed']=True

 doc.WingControls.setExpression('HalfSpan','ReferenceDimensions.HalfSpan')
 servo=doc.MG90SStaticEnvelope
 servo.addProperty('App::PropertyBool','NonphysicalKeepout','Wright');servo.NonphysicalKeepout=True
 servo.Label='Provisional MG90S static packaging envelope; nonphysical, no horn/access model'
 doc.recompute();physical=[n for n in prior['physical'] if n not in ['WingRight','WingLeft']]+wing_physical
 for obj in doc.Objects:
  if 'Visibility' in obj.PropertiesList:obj.Visibility=obj.Name in physical
 contacts=list(prior['contacts'])
 for side in ['Right','Left']:
  for i in range(6):contacts.extend([[side+'Skin',side+f'Rib{i}'],[side+'Spar',side+f'Rib{i}']])
 report.update(physical=physical,contacts=contacts,nonphysical_envelopes=['MG90SStaticEnvelope'],
     native_candidate=str(OUT/'proposed-unverified.FCStd'),span_control='ReferenceDimensions.HalfSpan drives WingControls.HalfSpan')
 doc.saveAs(report['native_candidate'])
phase('Rendering actual non-ruled candidate surfaces; fit not yet approved')
try:
 report['render']=surface_preview.selected([doc.getObject(n) for n in physical],OUT/'aircraft-proposed.png',width=640,height=480)
 (OUT/'render.json').write_text(json.dumps(report['render'],indent=2))
except ValueError as error:
 report.setdefault('render_failures',[]).append(str(error))
 print('Preview failure recorded; continuing geometry checks:',error,flush=True)
phase('Verifying source-point section fidelity on actual non-ruled outer surface')
residuals=[]
for section in doc.RightOuterContour.Sections:
 points=json.loads(section.WrightSplineSourceLocalPoints)
 distances=[Part.Vertex(section.Placement.multVec(App.Vector(*p))).distToShape(doc.RightOuterContour.Shape.Shells[0])[0] for p in points]
 residuals.append(dict(section=section.Name,points=len(points),max_source_point_distance_mm=max(distances)))
assert max(r['max_source_point_distance_mm'] for r in residuals)<=1e-5,residuals
report['outer_section_fidelity']=residuals
phase('Verifying cavity, true minimum wall, spar and unchanged aircraft parts')
outer=doc.RightOuterContour.Shape;cavity=doc.RightCavity.Shape;skin=doc.RightSkin.Shape
outside=cavity.cut(outer).Volume;wall=cavity.distToShape(outer.Shells[0])[0]
report['cavity']=dict(outside_mm3=outside,true_minimum_wall_mm=wall,required_wall_mm=.8)
assert outside<=1e-6 and wall>=.8,report['cavity']
spar_outside=doc.RightSpar.Shape.cut(cavity).Volume;assert spar_outside<=1e-6
report['spar_outside_cavity_mm3']=spar_outside
report['unchanged_other_parts']={n:hashlib.sha256(doc.getObject(n).Shape.exportBrepToString().encode()).hexdigest()==h for n,h in original.items()}
assert all(report['unchanged_other_parts'].values())
phase('Rechecking the same MG90S envelope against non-ruled cavity')
grown=Part.makeBox(33.1,33.5,13.4,servo.Placement.Base-App.Vector(.5,.5,.5))
servo_out=grown.cut(cavity).Volume;servo_gap=servo.Shape.distToShape(cavity.Shells[0])[0]
servo_overlap={n:grown.common(doc.getObject(n).Shape).Volume for n in ['RightSpar']+[f'RightRib{i}' for i in range(6)]}
report['servo_static_fit']=dict(origin_mm=list(servo.Placement.Base),size_mm=[32.1,32.5,12.4],per_side_gap_mm=.5,
    inflated_outside_cavity_mm3=servo_out,actual_wall_gap_mm=servo_gap,obstacle_overlap_mm3=servo_overlap)
assert servo_out<=1e-6 and servo_gap>=.5-1e-6 and max(servo_overlap.values())<=1e-6,report['servo_static_fit']
phase('Checking existing rotor envelopes against the changed wing geometry')
rotor_rows=[]
for sweep_name in ['SweepFR','SweepFL','SweepRR','SweepRL','PusherSweep']:
 sweep=doc.getObject(sweep_name)
 if sweep is None:continue
 a=sweep.Shape;ba=native.shape_bounds(a,conservative=True)
 for name in [n for n in physical if n.startswith(('RightSkin','LeftSkin','RightSpar','LeftSpar','RightRib','LeftRib'))]:
  b=doc.getObject(name).Shape;bb=native.shape_bounds(b,conservative=True)
  gaps=[max(0,getattr(ba,t+'Min')-getattr(bb,t+'Max'),getattr(bb,t+'Min')-getattr(ba,t+'Max')) for t in 'XYZ']
  lower=math.sqrt(sum(v*v for v in gaps))
  if lower>1e-6:
   row=dict(sweep=sweep_name,part=name,broadphase_separated=True,clearance_lower_bound_mm=lower,overlap_mm3=0.0)
  else:
   volume=a.common(b).Volume;distance=a.distToShape(b)[0]
   row=dict(sweep=sweep_name,part=name,broadphase_separated=False,distance_mm=distance,overlap_mm3=volume)
  rotor_rows.append(row)
report['rotor_vs_changed_wing']=rotor_rows
assert all(p['overlap_mm3']<=1e-6 and (p.get('broadphase_separated') or p['distance_mm']>1e-6) for p in rotor_rows),[p for p in rotor_rows if p['overlap_mm3']>1e-6 or (not p.get('broadphase_separated') and p['distance_mm']<=1e-6)]
phase('Checking full aircraft interference and required rib/skin/spar contacts')
report['fit']=worker.check(physical,contacts,tolerance_mm=.001,require_connected=False)
assert report['fit']['passed'],[(p['a'],p['b'],p['overlap_mm3'],p['distance_mm']) for p in report['fit']['pairs'] if p['clash'] or p['floating']]
phase('Saving verified native assembly and GLB; STEP handled separately if requested')
# Preserve the valid native dependency graph immediately; avoid making preview
# availability depend on a lengthy STEP equivalence run.
doc.saveAs(str(OUT/'assembly.FCStd'))
import exchange
report['native']=str(OUT/'assembly.FCStd')
report['glb']=exchange.export_glb([doc.getObject(n) for n in physical],OUT/'assembly.glb',report['fit'])
assert all(hashlib.sha256(Path(p).read_bytes()).hexdigest()==h for p,h in source_hashes.items())
report['source_preserved']=True;report['status']='baseline geometry verified; reopening pending'
(OUT/'result.json').write_text(json.dumps(report,indent=2))
App.closeDocument(doc.Name);worker._doc=None;worker.open_document(str(OUT/'assembly.FCStd'));doc=worker.active()
assert not doc.RightOuterContour.Ruled and bool(doc.RightCavity.Ruled)==report['native_loft_ruled']['RightCavity']
assert {n:len(doc.getObject(n).Sections) for n in report['source_section_counts']}==report['source_section_counts']
assert all(doc.getObject(n).Shape.isValid() for n in physical)
report['reopened_native_dependencies_passed']=True;report['status']='complete';report['elapsed_seconds']=time.time()-started
(OUT/'result.json').write_text(json.dumps(report,indent=2));phase('complete')
print('COMPLETE',flush=True)
