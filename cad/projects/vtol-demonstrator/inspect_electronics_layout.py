"""Independent package-v2 placement screen against immutable r4 native cavity."""
from pathlib import Path
import sys,json,hashlib,itertools
import FreeCAD as A,Part
ROOT=Path(__file__).resolve().parents[3];HERE=Path(__file__).resolve().parent
SOURCE=ROOT/'out/vtol-demonstrator-r5/assembly.FCStd';OUT=ROOT/'out/vtol-demonstrator-electronics-layout-r3';OUT.mkdir(exist_ok=True)
sys.path.insert(0,str(SOURCE.parent));import vtol_features_r5
sys.path.insert(0,str(HERE));import cots_import
sha=hashlib.sha256(SOURCE.read_bytes()).hexdigest();doc=A.openDocument(str(SOURCE));doc.recompute();cavity=doc.PodCavity.Shape
manifest=json.loads((SOURCE.parent/'artifact-manifest.json').read_text());physical=[doc.getObject(n) for n in manifest['physical']]
# Origin is conservative reservation lower XYZ, not a supplier mounting datum.
proposal=[('lift_battery',[152,56,43],[407,-28,2.5],None),('cruise_battery',[96.6,45.14,25.56],[275,-22.57,12],None),('fc',[54.3,39,17.5],[285,-19.5,40],'fc_current'),('pdb',[80,60,23.5],[190,-30,15],'pdb_top'),('pm02',[25,25,10],[350,-52,20],None),('ubec',[43.1,32.3,12.5],[320,25,20],None)]
def bb(s):
 b=s.optimalBoundingBox(False,False);return [b.XMin,b.YMin,b.ZMin,b.XMax,b.YMax,b.ZMax]
def overlap_bounds(a,b):return all(a[k]<b[k+3]-1e-7 and b[k]<a[k+3]-1e-7 for k in range(3))
def world(o):
 s=o.Shape.copy();s.Placement=o.getGlobalPlacement().multiply(o.Placement.inverse()).multiply(s.Placement);return s
results=[];boxes={};instances={}
for ident,size,origin,key in proposal:
 shape=Part.makeBox(*size,A.Vector(*origin));boxes[ident]=shape
 outside=shape.cut(cavity).Volume;gap=shape.distToShape(cavity)[0] # containment makes this zero; wall gap computed below
 shell=doc.Pod.Shape;wallgap=shape.distToShape(shell)[0]
 inflated=Part.makeBox(*[x+2 for x in size],A.Vector(*[x-1 for x in origin]));inflatedoutside=inflated.cut(cavity).Volume
 row={'id':ident,'reservation_size_mm':size,'reservation_lower_xyz_mm':origin,'outside_actual_cavity_mm3':outside,'inside':outside<1e-5,'pod_material_gap_mm':wallgap,'hatch_material_gap_mm':shape.distToShape(doc.Hatch.Shape)[0],'one_mm_service_reservation_inside':inflatedoutside<1e-5,'one_mm_service_outside_mm3':inflatedoutside,'geometry':'manufacturer_dimensional_reservation','existing_collisions':[]}
 for old in physical:
  if overlap_bounds(bb(shape),bb(old.Shape)):
   v=shape.common(old.Shape).Volume
   if v>1e-5:row['existing_collisions'].append({'name':old.Name,'volume_mm3':v})
 if key:
  group=cots_import.import_component(doc,key,'Candidate_'+ident,A.Placement(A.Vector(),A.Rotation(A.Vector(0,0,1),90 if key=='pdb_top' else 0)));leaves=cots_import.physical_objects(group);bounds=bb(Part.makeCompound([world(o) for o in leaves]));delta=A.Vector(*[origin[i]-bounds[i] for i in range(3)]);group.Placement=A.Placement(delta,group.Placement.Rotation);doc.recompute();actual=Part.makeCompound([world(o) for o in leaves]);ab=bb(actual)
  assert all(ab[i]>=origin[i]-1e-5 and ab[i+3]<=origin[i]+size[i]+1e-5 for i in range(3))
  row.update(geometry='verified_manufacturer_STEP_inside_tested_reservation',model_key=key,source_sha256=group.COTSSourceSHA256,source_to_world={'translation_mm':list(delta),'quaternion_xyzw':list(group.Placement.Rotation.Q)},actual_bounds_mm=ab,solid_count=len(leaves));instances[ident]=group
 results.append(row);print(ident,row['inside'],row['one_mm_service_reservation_inside'],row['existing_collisions'],flush=True)
pairs=[]
for a,b in itertools.combinations(boxes,2):
 s,t=boxes[a],boxes[b];v=s.common(t).Volume if overlap_bounds(bb(s),bb(t)) else 0
 pairs.append({'a':a,'b':b,'overlap_mm3':v,'gap_mm':s.distToShape(t)[0]})
# GPS is external reference only, no invented attachment. Import its actual housing.
gps=cots_import.import_component(doc,'gps_shared_full','Candidate_GPS',A.Placement(A.Vector(200,0,75),A.Rotation(A.Vector(1,0,0),90)))
gpsshape=Part.makeCompound([world(o) for o in cots_import.physical_objects(gps)])
gpsclashes=[]
for o in physical:
 if overlap_bounds(bb(gpsshape),bb(o.Shape)):
  v=gpsshape.common(o.Shape).Volume
  if v>1e-5:gpsclashes.append({'name':o.Name,'volume_mm3':v})
# Fourth verified source is inspected separately; servos belong in wing/control layout.
inspection=A.newDocument('ServoCoverage');sv=cots_import.import_component(inspection,'hs65mg','ServoSource',A.Placement());A.closeDocument(inspection.Name)
static_pass=all(x['inside'] and not x['existing_collisions'] for x in results) and all(x['overlap_mm3']<1e-5 for x in pairs) and not gpsclashes
passed=all(x['inside'] and x['one_mm_service_reservation_inside'] and not x['existing_collisions'] for x in results) and all(x['overlap_mm3']<1e-5 and x['gap_mm']>=2-1e-6 for x in pairs)
report={'source':str(SOURCE),'source_sha256':sha,'source_module':'vtol_features_r5','cavity_object':'PodCavity','cavity_valid':cavity.isValid(),'candidate_status':'static_fit_only_service_clearance_pending' if static_pass else 'infeasible_at_tested_placements','static_fit_passed':static_pass,'all_selected_inside_with_one_mm_service':passed and not gpsclashes,'components':results,'pairs':pairs,'gps_external':{'source_model':'gps_shared_full','translation_mm':[200,0,75],'rotation_axis':[1,0,0],'rotation_degrees':90,'bounds_mm':bb(gpsshape),'collisions':gpsclashes,'mount_verified':False},'coverage':{'internal_selected':['lift_battery','cruise_battery','fc','pdb','pm02','ubec'],'four_verified_source_models_imported':True,'servos':'Source inspected in separatedoc; no wingservo placement proposed','not_in_this_fit':['payload250g envelope','receiver','telemetryradio','airspeed','power/signal harness','connectors','batterystraps','PDB/FC/PM02/BEC mounts']},'limitations':['1mm inflated reservation is static clearance, not insertion/removal-path proof','Existing hatch and tray retained; source unchanged','Actual importedCAD bounded bytestedconservativeboxes; PM02/BEC/batteries remain dimensionalreferences','GPS candidate exterior placement has no attachment/loadpath qualification','No completeelectronicsintegration or CG/thermal claim']}
assert hashlib.sha256(SOURCE.read_bytes()).hexdigest()==sha
(OUT/'result.json').write_text(json.dumps(report,indent=2)+'\n');(OUT/'electronics-layout-candidate.json').write_text(json.dumps(report,indent=2)+'\n');print('RESULT',passed,flush=True)
