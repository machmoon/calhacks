import json
from pathlib import Path
import FreeCAD as App
import Part
root=Path(__file__).resolve().parents[2]
doc=App.openDocument(str(root/'out/freecad-aircraft-structural/assembly.FCStd'))
o=doc.getObject('part_pod')
print('POD',o.TypeId,o.Shape.Volume,len(o.Shape.Faces),[v.Name for v in o.OutList])
for n in ['part_pod','part_wing_R','part_rib_R1','part_pusher_mount']:
 o=doc.getObject(n);print(n,o.TypeId,o.Shape.Volume,str(o.Shape.BoundBox))
for n in ['part_pod_Outer','part_pod_Inner']:
 o=doc.getObject(n)
 if o:print(n,o.TypeId,o.Shape.Volume)
