"""Read-only transient geometry diagnostic; not an editable build/API verdict."""
import hashlib,json,math,time
from pathlib import Path
import FreeCAD as App
import Part
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/tsung-vtol-wing-pod-joint-batched'
source=ROOT/'out/tsung-vtol-smooth-aircraft-final/assembly.FCStd';digest=hashlib.sha256(source.read_bytes()).hexdigest();doc=App.openDocument(str(source));start=time.time()
probe=json.loads((OUT/'source-probe.json').read_text());result={'scope':'Independent transient-shape mechanical preflight only; no editable native output, typed action or structural success claim','source_sha256':digest,'stations':[]}
def cyl(r,h,x,y,z):return Part.makeCylinder(r,h,App.Vector(x,y,z))
def hexagon(x,y,z,af,h):
 r=af/math.sqrt(3);v=[App.Vector(x+r*math.cos(k*math.tau/6),y+r*math.sin(k*math.tau/6),z) for k in range(6)]
 return Part.Face(Part.makePolygon(v+[v[0]])).extrude(App.Vector(0,0,h))
def shift(s,v):
 c=s.copy();c.translate(App.Vector(*v));return c
def write(): (OUT/'mechanical-preflight.json').write_text(json.dumps(result,indent=2))
for x in [285,370]:
 print('Transient preflight station',x,flush=True)
 r=next(r for r in probe['vertical_intersections_mm'] if r['x']==x and r['y']==30);z=r['RightOuterContour'][0];roof=r['PodClosedShell_Outer'][-1]
 skin=doc.RightSkin.Shape.fuse(cyl(7,12,x,30,z-3).common(doc.RightOuterContour.Shape))
 skin=skin.cut(cyl(1.7,14,x,30,z-3)).cut(hexagon(x,30,z+4.6,5.7,2.6)).cut(Part.makeBox(6.6,35,2.6,App.Vector(x-3.3,-5,z+4.6)))
 seat=cyl(9,z-roof+18,x,30,roof-15).cut(doc.RightOuterContour.Shape)
 pod=doc.Pod.Shape.fuse(seat).cut(cyl(1.7,40,x,30,z-20)).cut(cyl(3.1,z+118,x,30,-130))
 screw=cyl(1.5,20,x,30,z-12).fuse(cyl(2.75,3,x,30,z-15)).cut(hexagon(x,30,z-15.1,2.5,1.4))
 nut=hexagon(x,30,z+4.6,5.5,2.4).cut(cyl(1.5,4.4,x,30,z+3.6))
 key=cyl(1.5,110.75,x,30,z-125.75).fuse(hexagon(x,30,z-15,2.5,1.25)).fuse(cyl(22,6,x,30,z-125.75))
 rotated=nut.copy();rotated.rotate(App.Vector(x,30,z),App.Vector(0,0,1),6)
 intersection=seat.common(skin)
 row=dict(x=x,skin_valid=skin.isValid(),skin_solids=len(skin.Solids),pod_valid=pod.isValid(),pod_solids=len(pod.Solids),seat_gap_mm=seat.distToShape(skin)[0],seat_overlap_mm3=intersection.Volume,seat_contact_area_mm2=intersection.Area,
   screw_skin_overlap_mm3=screw.common(skin).Volume,screw_pod_overlap_mm3=screw.common(pod).Volume,nut_skin_overlap_mm3=nut.common(skin).Volume,
   head_bearing_test_mm3=shift(screw,[0,0,.1]).common(pod).Volume,nut_floor_test_mm3=shift(nut,[0,0,-.1]).common(skin).Volume,nut_antirotation_test_mm3=rotated.common(skin).Volume,
   lateral_bearing_mm3=[shift(skin,[0,dy,0]).common(screw).Volume for dy in [-.3,.3]],
   key_overlaps_mm3={n:key.common(s).Volume for n,s in [('pod',pod),('skin',skin),('screw',screw),('nut',nut)]})
 result['stations'].append(row);write();print(row,flush=True)
result.update(elapsed_seconds=time.time()-start,source_file_unchanged=hashlib.sha256(source.read_bytes()).hexdigest()==digest,status='diagnostic complete; authoritative full assembly audit pending')
write()
