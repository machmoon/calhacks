"""Conservative full-turn envelope from actual propeller geometry and hub axis."""
import math
import FreeCAD as App
import Part
import native
from assembly_frames import world_shape

_BAND_CACHE={}
def refined_motor_envelope(local, placement, obstacle, source_key=None):
 # Each radial band encloses the actual native material in that band at every
 # angle. This refines the coarse disk; it never exempts the motor from collision.
 motor=obstacle.copy();motor.Placement=placement.inverse().multiply(motor.Placement)
 mb=native.shape_bounds(motor,conservative=True)
 reach=math.ceil((math.hypot(max(abs(mb.XMin),abs(mb.XMax)),max(abs(mb.YMin),abs(mb.YMax)))+.01)*1000)/1000
 b=native.shape_bounds(local,conservative=True);rows=[];volume=0
 cache_key=(source_key,round(reach,8)) if source_key else None
 cached=_BAND_CACHE.get(cache_key) if cache_key else None
 if cached is not None:
  for swept,row in cached:
   overlap=swept.common(motor).Volume;volume+=overlap;rows.append(dict(row,overlap_mm3=overlap))
  return {'overlap_mm3':volume,'radial_bands':rows,'method':'Cached source-identical conservative radial bands; motor transform/intersection rechecked'}
 cached=[]
 inner=3.0
 while inner<reach:
  outer=min(inner+2,reach)
  band=Part.makeCylinder(outer,b.ZLength+2,App.Vector(0,0,b.ZMin-1)).cut(Part.makeCylinder(inner,b.ZLength+2,App.Vector(0,0,b.ZMin-1)))
  material=local.common(band)
  if not material.isNull() and material.Volume>1e-7:
   bounds=native.shape_bounds(material,conservative=True)
   swept=Part.makeCylinder(outer,bounds.ZLength,App.Vector(0,0,bounds.ZMin)).cut(Part.makeCylinder(inner,bounds.ZLength,App.Vector(0,0,bounds.ZMin)))
   overlap=swept.common(motor).Volume;volume+=overlap
   row={'inner_radius_mm':inner,'outer_radius_mm':outer,'z_min_mm':bounds.ZMin,'z_max_mm':bounds.ZMax,'overlap_mm3':overlap};rows.append(row);cached.append((swept,row))
  inner=outer
 if cache_key:_BAND_CACHE[cache_key]=cached
 return {'overlap_mm3':volume,'radial_bands':rows,'method':'Conservative native-material radial bands, 2 mm maximum width; own motor remains tested; no allowed-contact exemption'}

def check(doc,objects):
 props=[doc.getObject(n) for n in objects if n.startswith('LiftProp')]
 if len(props)!=4:raise ValueError('Expected exactly four lift propeller instances')
 rows=[]
 for prop in props:
  placement=prop.getGlobalPlacement();local=world_shape(prop);local.Placement=placement.inverse().multiply(local.Placement)
  # Verify the known P12x4 central 6mm bore in native geometry, not a bbox axis guess.
  hubs=[]
  for face in local.Faces:
   surface=face.Surface
   if type(surface).__name__!='Cylinder':continue
   if abs(surface.Radius-3)<1e-5 and abs(surface.Center.x)<1e-5 and abs(surface.Center.y)<1e-5 and abs(abs(surface.Axis.z)-1)<1e-8:hubs.append(surface)
  if not hubs:raise ValueError(prop.Name+': independently expected central hub cylinder absent')
  b=native.shape_bounds(local,conservative=True)
  radius=math.hypot(max(abs(b.XMin),abs(b.XMax)),max(abs(b.YMin),abs(b.YMax)))
  shaft_void=Part.makeCylinder(3,b.ZLength,App.Vector(0,0,b.ZMin))
  void_overlap=local.common(shaft_void).Volume
  if void_overlap>1e-5:raise ValueError(prop.Name+': central shaft exclusion intersects actual propeller material')
  sweep=Part.makeCylinder(radius,b.ZLength,App.Vector(0,0,b.ZMin)).cut(shaft_void);sweep.Placement=placement
  suffix=prop.Name[len('LiftProp'):]
  excluded=[prop.Name]
  hits=[];tested=[];refinements=[]
  for name in objects:
   if name in excluded:continue
   obj=doc.getObject(name);tested.append(name)
   obstacle=world_shape(obj)
   if not sweep.BoundBox.intersect(obstacle.BoundBox):continue
   volume=sweep.common(obstacle).Volume
   if volume>1e-5 and name=='LiftMotor'+suffix:
    import hashlib
    canonical=prop.Shape.cleaned();canonical.Placement=App.Placement()
    source_key=hashlib.sha256(canonical.exportBrepToString().encode()).hexdigest()
    refined=refined_motor_envelope(local,placement,obstacle,source_key);refinements.append(dict(object=name,coarse_overlap_mm3=volume,**refined));volume=refined['overlap_mm3']
   if volume>1e-5:hits.append({'object':name,'overlap_mm3':volume})
  axis=placement.Rotation.multVec(App.Vector(0,0,1))
  rows.append({'propeller':prop.Name,'axis_world':list(axis),'center_world':list(placement.Base),'radius_bound_mm':radius,'central_void_radius_mm':3,'central_void_source_overlap_mm3':void_overlap,'axial_local_limits_mm':[b.ZMin,b.ZMax],'hub_cylinders_verified':len(hubs),'excluded_own_hardware':excluded,'tested_objects':tested,'clashes':hits,'motor_refinements':refinements,'clear':not hits})
 return {'passed':all(row['clear'] for row in rows),'checks':rows,'scope':'Conservative full360 annular envelope enclosing native propeller at every angle; intersection is potential interference, not exact swept-blade penetration. Only own propeller excluded; own motor, mounts and fasteners tested. Central shaft void excluded only after proving zero overlap with actual propeller material. No cruise propeller, handedness, vibration, deflection or flight claim.'}
