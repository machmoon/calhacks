"""Source-CAD mounting access gauges; no selected screw or accepted mount design."""
import hashlib,json,time,sys
from pathlib import Path
import FreeCAD as App
import Part
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/vendor-component-inventory';OUT.mkdir(parents=True,exist_ok=True)
sys.path.insert(0,str(ROOT/'cad/freecad'));import reference_clearance
config=json.loads((ROOT/'cad/projects/agents_in_space/references/jetson-cad-hole-candidates.json').read_text());path=Path(config['source']);expected=config['source_sha256'];assert hashlib.sha256(path.read_bytes()).hexdigest()==expected
start=time.perf_counter();shape=Part.read(str(path));entities=[]
def visit(s):
 if s.ShapeType in ('Solid','Shell','Face'):entities.append(s)
 elif s.ShapeType in ('Compound','CompSolid'):
  for c in s.childShapes():visit(c)
 else:raise ValueError('unexpected source entity '+s.ShapeType)
visit(shape);solids=[s for s in entities if s.ShapeType=='Solid'];references=[s for s in entities if s.ShapeType!='Solid']
assert len(solids)==1393 and len(references)==527
bounds=[s.BoundBox for s in solids];refbounds=[s.BoundBox for s in references]
def overlap(a,b):return all(getattr(a,k+'Max')>=getattr(b,k+'Min')-1e-6 and getattr(b,k+'Max')>=getattr(a,k+'Min')-1e-6 for k in 'XYZ')
rows=[]
for index,h in enumerate(config['holes']):
 x,y,z=h['center_mm'];gauges=[('shaft_diameter_2p5',Part.makeCylinder(1.25,18,App.Vector(x,y,-10))),('underside_support_OD5_ID2p75',Part.makeCylinder(2.5,6,App.Vector(x,y,-6)).cut(Part.makeCylinder(1.375,6,App.Vector(x,y,-6)))),('top_tool_diameter5',Part.makeCylinder(2.5,23.43,App.Vector(x,y,1.57)))]
 for label,g in gauges:
  assert g.isValid() and len(g.Solids)==1
  collisions=[];errors=[];nearrefs=[];reference_evidence=[];tested=0
  for i,(s,b) in enumerate(zip(solids,bounds)):
   if not overlap(b,g.BoundBox):continue
   tested+=1
   try:
    common=s.common(g)
    if not common.isNull() and not common.isValid():raise ValueError('invalid intersection')
    if common.Volume>1e-6:collisions.append(dict(source_solid_index=i+1,overlap_mm3=common.Volume))
   except Exception as e:errors.append(dict(source_solid_index=i+1,error=str(e)))
  for i,(s,b) in enumerate(zip(references,refbounds)):
   if not overlap(b,g.BoundBox):continue
   try:
    finding=reference_clearance.obstruction(s,g)
    reference_evidence.append(dict(source_reference_index=i+1,**finding))
    if finding['obstructed']:nearrefs.append(i+1)
   except Exception as e:errors.append(dict(source_reference_index=i+1,error=str(e)))
  rows.append(dict(hole_index=index+1,xy_mm=[x,y],gauge=label,physical_candidates_tested=tested,physical_collisions=collisions,intersecting_reference_surfaces=nearrefs,reference_obstruction_evidence=reference_evidence,errors=errors,geometric_gauge_clear=not collisions and not nearrefs and not errors))
  (OUT/'jetson-mount-access-progress.json').write_text(json.dumps(dict(completed_gauges=len(rows),total_gauges=24,elapsed_seconds=time.perf_counter()-start,last=rows[-1]),indent=2))
assert hashlib.sha256(path.read_bytes()).hexdigest()==expected
report=dict(reference_enclosure_checked=True,source_sha256=expected,source_unchanged=True,source_solids=len(solids),source_references=len(references),gauges=rows,elapsed_seconds=time.perf_counter()-start,scope='Provisional axial clearance gauges in original STEP frame. 2.5mm shaft, OD5mm support annulus, diameter5mm top tool. No actual screw, standoff, connector or tool selected. Reference-surface contacts and containment inside closed reference enclosures block clear claims. This does not establish source-wide validity, structural support, lateral tool approach, installed cables or actual hardware fit.')
(OUT/'jetson-mount-access.json').write_text(json.dumps(report,indent=2));print(json.dumps(dict(completed=len(rows),clear=sum(r['geometric_gauge_clear'] for r in rows),elapsed_seconds=report['elapsed_seconds'])))
