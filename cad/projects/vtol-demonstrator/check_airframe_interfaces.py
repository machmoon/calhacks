"""Read-only kernel checks of the previously failing airframe interfaces."""
import FreeCAD as A,Part,json,sys,hashlib
from pathlib import Path
out=Path(__file__).resolve().parents[3]/'out/vtol-demonstrator-r4';sys.path.insert(0,str(out));import vtol_features_r4
file=out/'assembly.FCStd';sha=hashlib.sha256(file.read_bytes()).hexdigest();d=A.openDocument(str(file));rows=[]
for side in ['Right','Left']:
 pairs=[(side+'Spar',side+'WingSkin'),('Boom'+side,'Fin'+side)]+[(side+'Rib'+str(i),side+'Aileron') for i in range(2,6)]
 for a,b in pairs:
  x=d.getObject(a).Shape;y=d.getObject(b).Shape;row=dict(pair=[a,b],common_mm3=x.common(y).Volume,distance_mm=x.distToShape(y)[0]);rows.append(row);print(row,flush=True)
outer=d.RightOuterContour.Shape;tube=d.SparOuter.Shape;surfaces=Part.makeCompound([f for f in outer.Faces if not isinstance(f.Surface,Part.Plane)]);passage=d.SparPassage.Shape
r=dict(revision='layout-r4-airframe-interfaces',source_sha256=sha,source_unchanged=sha==hashlib.sha256(file.read_bytes()).hexdigest(),checks=rows,passed=all(row['common_mm3']<1e-6 for row in rows),spar_outside_outer_mm3=tube.cut(outer).Volume,spar_to_aerodynamic_surface_min_mm=tube.distToShape(surfaces)[0],passage_to_aerodynamic_surface_min_mm=passage.distToShape(surfaces)[0],limitations=['No strength/preload qualification','Control surfaces remain unhinged; motion and retention not verified','Source outer preserved; root spar passage deliberately opens root cap'])
(out/'airframe-interface-check.json').write_text(json.dumps(r,indent=2));print(r,flush=True)
