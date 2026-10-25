"""Fit native cavity profiles to measured sections of the actual smooth exterior.

These are explicit construction snapshots, not a live constant-wall feature.
All source section data and fitting tolerances are retained in the native model.
"""
import bisect,hashlib,json,math,sys,time
from pathlib import Path
import FreeCAD as App
import Part
import native,profiles,worker
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/tsung-vtol-smooth-wing-actual-sections';OUT.mkdir(parents=True,exist_ok=True)
SOURCE=ROOT/'out/tsung-vtol-smooth-aircraft-ruled-cavity/wing-candidate.FCStd'
hash_before=hashlib.sha256(SOURCE.read_bytes()).hexdigest();started=time.time()
report=dict(source=str(SOURCE),source_sha256=hash_before,status='building',nominal_section_inset_mm=1.2,
    method='Actual non-ruled outer-solid sections, native inward offset, two-curve fitted native snapshots, ruled cavity interpolation',
    live_constant_wall_feature=False,exact_source_outer_changed=False,engineering_assumptions=True)
def phase(s):
 report['status']=s;(OUT/'progress.json').write_text(json.dumps(report,indent=2));print(s,flush=True)
def failed(kind,error,trace):
 report['status']='failed';report['failure']=str(error);(OUT/'failure.json').write_text(json.dumps(report,indent=2));sys.__excepthook__(kind,error,trace)
sys.excepthook=failed
worker.open_document(str(SOURCE));doc=worker.active();outer=doc.RightOuterContour
outer_before=hashlib.sha256(outer.Shape.exportBrepToString().encode()).hexdigest()
etas=[.005,.04,.08,.12,.18,.25,.325,.4,.475,.55,.625,.7,.77,.82,.86,.9]
rings=[];evidence=[]
for eta in etas:
 phase('Extracting actual outer section eta='+str(eta))
 plane=Part.makePlane(1000,1000,App.Vector(0,600*eta,-300),App.Vector(0,1,0))
 # Plane orientation is deliberately checked through the actual resulting wire.
 section=outer.Shape.section(plane)
 groups=Part.sortEdges(section.Edges)
 if len(groups)!=1:raise ValueError('Outer section did not yield one connected wire: '+str(eta))
 wire=Part.Wire(groups[0])
 if not wire.isClosed() or not wire.isValid():raise ValueError('Invalid actual outer section')
 bounds=native.shape_bounds(wire)
 if abs(bounds.YMin-600*eta)>1e-6 or abs(bounds.YMax-600*eta)>1e-6:raise ValueError('Section plane orientation mismatch')
 offset_source='actual_native_section'
 polygon_evidence=None
 try:
  offset=wire.makeOffset2D(-1.2,join=0,fill=False)
 except ValueError:
  # OCC can reject offsets of the high-degree intersection curves. Use a
  # measured planar polygon construction approximation, never a hidden fallback.
  raw=wire.discretize(Number=4001)
  if (raw[-1]-raw[0]).Length<1e-7:raw=raw[:-1]
  k=max(range(len(raw)),key=lambda i:raw[i].x);raw=raw[k:]+raw[:k]
  le=min(range(len(raw)),key=lambda i:raw[i].x)
  def polygon_branch(branch):
   lengths=[0.0]
   for a,b in zip(branch,branch[1:]):lengths.append(lengths[-1]+(b-a).Length)
   output=[]
   for j in range(257):
    target=lengths[-1]*(1-math.cos(math.pi*j/256))/2
    index=min(len(branch)-2,max(0,bisect.bisect_right(lengths,target)-1))
    f=(target-lengths[index])/(lengths[index+1]-lengths[index])
    output.append(branch[index]+(branch[index+1]-branch[index])*f)
   return output
  upper_poly=polygon_branch(raw[:le+1]);lower_poly=polygon_branch(raw[le:]+raw[:1]);poly_points=upper_poly+lower_poly[1:-1]
  polygon=Part.makePolygon(poly_points+poly_points[:1])
  if not polygon.isValid() or not polygon.isClosed() or not Part.Face(polygon).isValid():raise ValueError('Invalid section construction polygon')
  poly_errors=profiles._polyline_distances(raw,poly_points)
  poly_errors.extend(Part.Vertex(p).distToShape(wire)[0] for p in poly_points[::4])
  assert max(poly_errors)<=.03,max(poly_errors)
  polygon_evidence=dict(vertices=len(poly_points),native_section_samples=len(raw),sampled_bidirectional_max_mm=max(poly_errors),certified_maximum_bound=False,reason='Native offset of high-degree actual section returned null')
  offset=polygon.makeOffset2D(-1.2,join=0,fill=False)
  offset_source='measured_polygon_approximation_of_actual_native_section'

 if offset.isNull() or not offset.isValid() or len(offset.Wires)!=1:raise ValueError('Invalid actual section inset')
 target=offset.Wires[0]
 if not target.isClosed() or Part.Face(target).Area>=Part.Face(wire).Area:raise ValueError('Inset failed section-area reduction')
 dense=target.discretize(Number=4001)
 if (dense[-1]-dense[0]).Length<1e-7:dense=dense[:-1]
 start=max(range(len(dense)),key=lambda i:dense[i].x);dense=dense[start:]+dense[:start]
 leading=min(range(len(dense)),key=lambda i:dense[i].x)
 if max(p.z for p in dense[:leading+1])<max(p.z for p in dense[leading:]):
  dense=dense[:1]+list(reversed(dense[1:]));leading=min(range(len(dense)),key=lambda i:dense[i].x)
 def sample(branch):
  distances=[0.0]
  for a,b in zip(branch,branch[1:]):distances.append(distances[-1]+(b-a).Length)
  points=[]
  for j in range(65):
   t=distances[-1]*(1-math.cos(math.pi*j/64))/2
   i=min(len(branch)-2,max(0,bisect.bisect_right(distances,t)-1));f=(t-distances[i])/(distances[i+1]-distances[i])
   points.append(branch[i]+(branch[i+1]-branch[i])*f)
  return points
 upper=sample(dense[:leading+1]);lower=sample(dense[leading:]+dense[:1]);points=upper+lower[1:-1]
 coords=[list(p) for p in points]
 curves,frame,_,quality=profiles._spline_data(coords,64,native.vector,.15)
 fitted=Part.Wire([c.toShape() for c in curves]);fitted.Placement=frame
 errors=[Part.Vertex(frame.multVec(p)).distToShape(target)[0] for c in curves for p in c.discretize(Number=201)]
 errors.extend(Part.Vertex(p).distToShape(fitted)[0] for p in dense[::20])
 assert max(errors)<=.08,max(errors)
 evidence.append(dict(eta=eta,source_section_edges=len(wire.Edges),offset_source=offset_source,section_polygon_approximation=polygon_evidence,offset_edges=len(target.Edges),sampled_bidirectional_offset_fit_max_mm=max(errors),certified_maximum_bound=False,profile_quality=quality))
 rings.append(coords)
phase('Building native cavity snapshots from actual sections')
new=profiles.spline_loft(doc,'ActualCavity',rings,64,native.vector,.15)
sections=list(new.Sections)
for section,eta in zip(sections,etas):
 section.setExpression('Placement.Base.y',f'WingControls.HalfSpan * {eta}')
 section.addProperty('App::PropertyString','ActualOuterSectionProvenance','Wright')
 section.ActualOuterSectionProvenance=json.dumps(dict(source_outer_sha256=outer_before,source_section_eta=eta,nominal_inset_mm=1.2,live_offset=False,regenerate_after_outer_profile_edits=True))
doc.RightCavity.Sections=sections;doc.RightCavity.Ruled=True
doc.RightCavity.WrightSplineRepresentation=new.WrightSplineRepresentation
doc.removeObject(new.Name);doc.recompute()
physical=['RightSkin','RightSpar']+[f'RightRib{i}' for i in range(6)];physical +=[n.replace('Right','Left') for n in physical]
contacts=[]
for side in ['Right','Left']:
 for i in range(6):contacts.extend([[side+'Skin',side+f'Rib{i}'],[side+'Spar',side+f'Rib{i}']])
report.update(section_fits=evidence,physical=physical,contacts=contacts,native_face_counts={n:len(doc.getObject(n).Shape.Faces) for n in physical+['RightOuterContour','RightCavity']})
native.validate_document(doc)
assert hashlib.sha256(outer.Shape.exportBrepToString().encode()).hexdigest()==outer_before
snapshot=OUT/'unverified-build.FCStd';doc.saveAs(str(snapshot));report['checkpoint']=str(snapshot)
phase('Checking actual cavity containment and true minimum wall')
def measure():
 current=worker.active();c=current.RightCavity.Shape;o=current.RightOuterContour.Shape
 outside=c.cut(o).Volume;distance=c.distToShape(o.Shells[0])
 result=dict(cavity_outside_mm3=outside,minimum_wall_mm=distance[0],wall_point_pairs=[[list(a),list(b)] for a,b in distance[1]])
 report['latest_wall']=result
 assert outside<=1e-6 and distance[0]>=.8,result
 spar_out=current.RightSpar.Shape.cut(c).Volume;assert spar_out<=1e-6
 servo=current.MG90SStaticEnvelope;grown=Part.makeBox(33.1,33.5,13.4,servo.Placement.Base-App.Vector(.5,.5,.5))
 out=grown.cut(c).Volume;gap=servo.Shape.distToShape(c.Shells[0])[0]
 overlaps={n:grown.common(current.getObject(n).Shape).Volume for n in ['RightSpar']+[f'RightRib{i}' for i in range(6)]}
 result.update(spar_outside_mm3=spar_out,servo=dict(inflated_outside_mm3=out,wall_gap_mm=gap,obstacle_overlap_mm3=overlaps))
 assert out<=1e-6 and gap>=.5-1e-6 and max(overlaps.values())<=1e-6,result['servo']
 return result
report['baseline']=measure();phase('Checking ribs/skin/spar contacts and interference')
report['fit']=worker.check(physical,contacts,tolerance_mm=.001);assert report['fit']['passed']
doc.saveAs(str(OUT/'assembly.FCStd'));report['native']=str(OUT/'assembly.FCStd')
phase('Reopening and verifying linked610mm span edit')
App.closeDocument(doc.Name);worker._doc=None;worker.open_document(report['native']);doc=worker.active()
native.set_parameter(doc,'WingControls','HalfSpan',610);report['reopened_610mm']=measure()
assert all(doc.getObject(n).Shape.isValid() for n in physical)
report['edited_fit']=worker.check(physical,contacts,tolerance_mm=.001);assert report['edited_fit']['passed']
worker.checkpoint(str(OUT/'edited-610mm'))
assert hashlib.sha256(SOURCE.read_bytes()).hexdigest()==hash_before
report.update(status='complete',source_preserved=True,outer_geometry_unchanged=True,elapsed_seconds=time.time()-started)
(OUT/'result.json').write_text(json.dumps(report,indent=2));phase('complete')
