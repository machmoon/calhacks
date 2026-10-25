"""Export exact CAD triangles for a labelled cutaway preview, never analysis."""
import json,sys
from pathlib import Path
import FreeCAD as App
import Part
root=Path(sys.argv[1]).resolve()
doc=App.openDocument(str(root/'assembly.FCStd'))
report=json.loads((root/'result.json').read_text())
faces=[];colors=[]
for part in report['physical_parts']:
    obj=doc.getObject(part['name']);shape=obj.Shape
    if obj.Name=='Skin' or obj.Name.startswith('part_wing_'):
        # Remove the upper portion for visibility; this does not edit the document.
        b=shape.BoundBox
        shape=shape.cut(Part.makeBox(b.XLength+20,b.YLength+20,60,App.Vector(b.XMin-10,b.YMin-10,8)))
        color='#83a9bd'
    elif obj.Name.startswith(('Rib','part_rib_')):color='#d7b17c'
    else:color='#485969'
    vertices,triangles=shape.tessellate(.12)
    faces.extend([[[vertices[i].x,vertices[i].y,vertices[i].z] for i in t] for t in triangles])
    colors.extend([color]*len(triangles))
(root/'preview-mesh.json').write_text(json.dumps(dict(faces=faces,colors=colors)))
App.closeDocument(doc.Name)
