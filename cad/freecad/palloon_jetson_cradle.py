"""Source-frame cradle proposal, checked against scoped vendor geometry only."""
import hashlib,json
from pathlib import Path
import FreeCAD as App
import native,worker
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/palloon-jetson-cradle';OUT.mkdir(parents=True,exist_ok=True)
source=ROOT/'out/jetson-obstruction-study/scoped-source-extract.FCStd';digest=hashlib.sha256(source.read_bytes()).hexdigest();worker.open_document(str(source));doc=worker.active();call=worker.METHODS;trace=[]
def act(action,**args):
 r=call[action](**args);trace.append(dict(action=action,args=args));return r
b=native.shape_bounds(doc.SourceSolid1267.Shape);length=b.XLength;width=b.YLength;gap=.3;wall=2.;ledge=3.;floor=3.;lip=3.
act('make_parameters',name='CradleDimensions',parameters={'Length':dict(value=length,min=length,max=length+10),'Width':dict(value=width,min=width,max=width+10)})
def box(name,x,y,z,pos):
 act('make_part',spec=dict(name=name,kind='box',params=dict(x=x,y=y,z=z)));act('place',object_name=name,position=pos)
box('CradleBlank',length+2*(gap+wall),width+2*(gap+wall),floor+lip,[b.XMin-gap-wall,b.YMin-gap-wall,b.ZMin-floor])
box('CradleCavity',length+2*gap,width+2*gap,lip+1,[b.XMin-gap,b.YMin-gap,b.ZMin])
act('boolean',name='CradleTray',operation='cut',base='CradleBlank',tool='CradleCavity')
box('CentreOpening',length-2*ledge,width-2*ledge,floor+lip+2,[b.XMin+ledge,b.YMin+ledge,b.ZMin-floor-1])
act('boolean',name='JetsonCradle',operation='cut',base='CradleTray',tool='CentreOpening')
bindings=[]
for obj,offset in [('CradleBlank',2*(gap+wall)),('CradleCavity',2*gap),('CentreOpening',-2*ledge)]:
 for target,source_parameter in [('Length','Length'),('Width','Width')]:bindings.append(dict(object_name=obj,parameter=target,source_object='CradleDimensions',source_parameter=source_parameter,offset_mm=offset))
act('bind_parameters',bindings=bindings)
names=['SourceSolid275','SourceSolid1267','JetsonCradle']
study=json.loads((ROOT/'out/jetson-obstruction-study/result.json').read_text())
for index in [275,1267]:
 o=doc.getObject('SourceSolid'+str(index))
 if 'WrightStepImport' not in o.PropertiesList:o.addProperty('App::PropertyString','WrightStepImport','WrightImport')
 o.WrightStepImport=json.dumps(dict(source_path=study['source'],source_sha256=study['source_sha256'],source_solid_count=1393,source_reference_surface_count=527,import_instance='diagnostic-jetson-scoped-carrier-frame',geometry_role='physical_solid',source_entity_index=index,source_solid_index=index,representation='Scoped diagnostic extraction, not complete vendor import'))
 o.setEditorMode('WrightStepImport',1)
r=act('check',objects=names,contacts=[['SourceSolid1267','JetsonCradle']],tolerance_mm=.01,summary=True)
# Preserve failures as actionable geometry evidence, not a manufactured success.
path=OUT/'proposal.FCStd';doc.saveAs(str(path))
assert hashlib.sha256(source.read_bytes()).hexdigest()==digest
report=dict(source=str(source),source_sha256=digest,source_unchanged=True,source_geometry_scope='Only carrier solid275 and underlying frame solid1267 from official STEP;1391other source solids and527references are absent.',geometry_check=r,parameters=dict(side_clearance_mm=gap,wall_mm=wall,ledge_mm=ledge,floor_mm=floor,lip_mm=lip),native=str(path),scope='Proposed open-centre cradle for the existing source frame. No retention hardware, full-electronics interference, cables, thermal adequacy, strength or actual user configuration verified. Not accepted payload assembly.')
(OUT/'result.json').write_text(json.dumps(report,indent=2));(OUT/'typed-actions.json').write_text(json.dumps(trace,indent=2));print(json.dumps(r,indent=2))
