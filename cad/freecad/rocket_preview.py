"""Export native rocket triangles for a labelled longitudinal cutaway preview."""
import json,os,sys
from pathlib import Path
sys.path.insert(0,os.environ['WRIGHT_ROCKET_WORKBENCH'])
import FreeCAD as App
import Part
report=Path(sys.argv[1]).resolve();index=int(sys.argv[2])
entry=json.loads(report.read_text())['imports'][index]
if not entry['import_passed']:raise ValueError('cannot preview a failed import')
doc=App.openDocument(entry['native_path']);doc.recompute()
faces=[];colors=[];shown=[]
for component in entry['component_coverage']:
    if component['status']!='created' or component['tag']=='stage':continue
    obj=doc.getObject(component['native_object']);shape=obj.Shape
    if abs(shape.Volume-component['native_volume_mm3'])>max(1e-5,shape.Volume*1e-7):raise ValueError('native reopen changed '+obj.Name)
    tag=component['tag'];color='#809faf'
    if tag in ['bodytube','nosecone','tubecoupler','innertube']:
        b=shape.BoundBox
        if b.YMin<0:shape=shape.cut(Part.makeBox(b.XLength+2,-b.YMin+1,b.ZLength+2,App.Vector(b.XMin-1,b.YMin-1,b.ZMin-1)))
    elif tag.endswith('finset'):color='#c28a55'
    elif tag=='railbutton':color='#344b5e'
    else:color='#d6b975'
    vertices,triangles=shape.tessellate(.15)
    faces.extend([[[vertices[i].x,vertices[i].y,vertices[i].z] for i in tri] for tri in triangles])
    colors.extend([color]*len(triangles));shown.append(obj.Name)
out=Path(entry['native_path']).parent
(out/'preview-mesh.json').write_text(json.dumps(dict(faces=faces,colors=colors,shown=shown,scope='Longitudinal cutaway of supported native components; front halves of tubes/nose removed only for display')))
App.closeDocument(doc.Name)
print('Native reopen and cutaway triangles verified:',len(shown),'component definitions')
