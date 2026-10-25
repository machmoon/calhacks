"""Proposed straight driver paths derived from native bolt head/shank links."""
import FreeCAD as App
import Part
import native
from assembly_frames import world_shape

def hatch_removal(doc,obstacles):
 hatch=doc.getObject('Hatch')
 if hatch is None:return {'passed':False,'reason':'No declared hatch'}
 shape=world_shape(hatch)
 # A translated solid at a sampled pose can prove a blocked path. Only a
 # disjoint conservative swept box proves a clear continuous translation.
 direction=getattr(hatch,'RemovalDirection',App.Vector(0,0,200))
 if direction.Length<1:raise ValueError('Hatch removal displacement is missing')
 b=native.shape_bounds(shape,conservative=True)
 start=App.Vector(b.XMin+min(0,direction.x),b.YMin+min(0,direction.y),b.ZMin+min(0,direction.z))
 swept_box=Part.makeBox(b.XLength+abs(direction.x),b.YLength+abs(direction.y),b.ZLength+abs(direction.z),start)
 candidates=[]
 for name in obstacles:
  if name=='Hatch':continue
  other=world_shape(doc.getObject(name))
  if swept_box.BoundBox.intersect(native.shape_bounds(other,conservative=True)):
   if swept_box.common(other).Volume>1e-5:candidates.append((name,other))
 hits=[]
 for name,other in candidates:
  for distance in [1,5,10,20,40,80,120,200]:
   moved=shape.copy();moved.translate(direction*(distance/200))
   if not moved.BoundBox.intersect(other.BoundBox):continue
   overlap=moved.common(other).Volume
   if overlap>1e-5:
    hits.append({'object':name,'distance_mm':distance,'overlap_mm3':overlap});break
 return {'passed':not candidates,'status':'blocked' if hits else 'inconclusive' if candidates else 'clear','part':'Hatch','direction_mm':list(direction),'clashes':hits,'method':'Conservative swept-box separation certifies clear translation; actual solid intersections at sampled poses prove blockage. Clear samples alone do not certify continuous clearance; no fastening release claim'}

def check(doc,physical,obstacles):
 rows=[];unassessed=[]
 removal=hatch_removal(doc,obstacles)
 service_obstacles=[n for n in obstacles if n!='Hatch'] if removal['passed'] else obstacles
 for name in physical:
  if not any(term in name for term in ['Bolt','Screw','Nut']):continue
  bolt=doc.getObject(name);children=list(getattr(bolt,'Shapes',[]))
  heads=[o for o in children if 'Head' in o.Name and o.TypeId=='Part::Cylinder']
  shafts=[o for o in children if 'Shank' in o.Name and o.TypeId=='Part::Cylinder']
  if len(heads)!=1 or len(shafts)!=1:
   unassessed.append({'name':name,'reason':'No unique linked native cylindrical head/shank; nut tool/removal routes require separate definition'});continue
  head,shaft=heads[0],shafts[0];placement=head.getGlobalPlacement();local=world_shape(head);local.Placement=placement.inverse().multiply(local.Placement)
  b=native.shape_bounds(local);axis=placement.Rotation.multVec(App.Vector(0,0,1));head_center=world_shape(head).CenterOfMass;shaft_center=world_shape(shaft).CenterOfMass
  projection=(head_center-shaft_center).dot(axis)
  if abs(projection)<1e-5:
   unassessed.append({'name':name,'reason':'Ambiguous outward driver direction'});continue
  sign=1 if projection>0 else -1;direction=axis*sign
  origin=placement.multVec(App.Vector((b.XMin+b.XMax)/2,(b.YMin+b.YMax)/2,b.ZMax if sign>0 else b.ZMin))
  tool=Part.makeCylinder(3,200,origin,direction);hits=[]
  for target in service_obstacles:
   shape=world_shape(doc.getObject(target))
   if not tool.BoundBox.intersect(shape.BoundBox):continue
   volume=tool.common(shape).Volume
   if volume>1e-5:hits.append({'object':target,'overlap_mm3':volume})
  rows.append({'fastener':name,'head':head.Name,'shank':shaft.Name,'origin_mm':list(origin),'direction':list(direction),'diameter_mm':6,'length_mm':200,'clashes':hits})
 return {'checks':rows,'removal_sequence':[removal],'service_state':'hatch_removed' if removal['passed'] else 'closed_hatch_removal_blocked','driver_paths_passed':bool(rows) and all(not row['clashes'] for row in rows),'passed':bool(rows) and all(not row['clashes'] for row in rows) and removal['passed'],'covered_driver_fasteners':[r['fastener'] for r in rows],'unassessed_fasteners':unassessed,'complete_service_coverage':False,'scope':'Proposed Ø6×200mm straight driver from actual linked head outward from shank; native placement/world frames respected. Declared hatch removal sequence checked; nut-tool, other removal, cable and reference-envelope occupancy not assessed; not exact purchased tool.'}
