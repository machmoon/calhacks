"""Native FreeCAD composition of editable battery bay and scoped Jetson cradle."""
import base64,hashlib,json
from pathlib import Path
import FreeCAD as App
import Part
import render
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/palloon-native-layout';OUT.mkdir(parents=True,exist_ok=True)
paths=[ROOT/'out/palloon-battery-bay/assembly.FCStd',ROOT/'out/palloon-jetson-cradle/proposal.FCStd'];hashes={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}
a=App.openDocument(str(paths[0]));b=App.openDocument(str(paths[1]));doc=App.newDocument('PalloonNativeLayout')
battery=doc.copyObject([a.BatteryBay,a.BatteryEnvelope,a.NonphysicalLeadKeepout,a.NonphysicalConnectorKeepout],True)
jetson=doc.copyObject([b.SourceSolid275,b.SourceSolid1267,b.JetsonCradle],True)
assert all(dep.Document==doc for o in doc.Objects for dep in o.OutList),'external dependency retained'
assert len([o for o in doc.Objects if o.Name.startswith('BatteryLayout')])==1
assert len([o for o in doc.Objects if o.Name.startswith('CradleDimensions')])==1
cradle=jetson[-1];bounds=cradle.Shape.optimalBoundingBox(False,False)
# Native multi-parameter expressions keep the full cradle group downstream of
# the battery bay and its external connector reserve as the bay is resized.
gap=55.;dy=-bounds.YMin;dz=-bounds.ZMin
for o in jetson:
 p=o.Placement.Base
 o.setExpression('Placement.Base.x',f'BatteryLayout.BayLength + 2 * BatteryLayout.Wall + {gap-bounds.XMin+p.x} mm')
 o.Placement.Base.y=p.y+dy;o.Placement.Base.z=p.z+dz
plate=doc.addObject('Part::Box','PayloadDeck');plate.Height=3;plate.Placement.Base=App.Vector(-5,-5,-3)
plate.setExpression('Length',f'BatteryLayout.BayLength + 2 * BatteryLayout.Wall + CradleDimensions.Length + {gap+2*(.3+2)+10} mm')
plate.setExpression('Width','CradleDimensions.Width + 14.6 mm')
physical=[battery[0],battery[1]]+list(jetson)+[plate]
for o in doc.Objects:
 if 'Visibility' in o.PropertiesList:o.Visibility=o in physical
notes=doc.addObject('App::FeaturePython','DesignScope');notes.addProperty('App::PropertyString','Scope');notes.Scope='Draft payload layout: battery envelope assumed; only Jetson carrier/frame source solids275/1267 included. Full electronics, retention, wiring and structural/thermal verification pending.'
doc.recompute();doc.saveAs(str(OUT/'assembly.FCStd'))
def fingerprint(objects):return hashlib.sha256(''.join(o.Shape.cleaned().exportBrepToString() for o in objects).encode()).hexdigest()
def state():
 doc.recompute();bb=battery[0].Shape.optimalBoundingBox(False,False);jb=cradle.Shape.optimalBoundingBox(False,False);pb=plate.Shape.optimalBoundingBox(False,False)
 assert all(o.Shape.isValid() and len(o.Shape.Solids)==1 for o in physical)
 assert abs(jb.XMin-bb.XMax-gap)<1e-6
 assert pb.XMax>jb.XMax and pb.YMax>jb.YMax
 assert all(dep.Document==doc for o in doc.Objects for dep in o.OutList)
 return dict(bay_length_mm=float(doc.BatteryLayout.BayLength),battery_to_cradle_gap_mm=jb.XMin-bb.XMax,deck_bounds_mm=[pb.XMin,pb.YMin,pb.ZMin,pb.XMax,pb.YMax,pb.ZMax],cradle_min_x_mm=jb.XMin,physical_solids=sum(len(o.Shape.Solids) for o in physical))
baseline=state();doc.BatteryLayout.BayLength=185;expanded=state();doc.BatteryLayout.BayLength=155;restored=state();doc.save()
r=render.selected(physical,fingerprint,width=1000,height=650,azimuth_degrees=-45,elevation_degrees=35,deflection_mm=.3,max_triangles=100000)
(OUT/'layout.png').write_bytes(base64.b64decode(r.pop('content_blocks')[0]['source']['data']));(OUT/'render.json').write_text(json.dumps(r,indent=2))
App.closeDocument(doc.Name);doc=App.openDocument(str(OUT/'assembly.FCStd'));assert abs(float(doc.BatteryLayout.BayLength)-155)<1e-8;assert all(dep.Document==doc for o in doc.Objects for dep in o.OutList)
assert all(hashlib.sha256(Path(p).read_bytes()).hexdigest()==h for p,h in hashes.items())
(OUT/'result.json').write_text(json.dumps(dict(status='native_layout_created',source_files_unchanged=True,sources=hashes,baseline=baseline,expanded=expanded,restored=restored,reopened=True,self_contained_native_dependencies=True,whole_payload_verified=False,scope=doc.DesignScope.Scope,limitations=['Battery size remains an assumption','1391 Jetson solids and527references omitted','Deck/bay/cradle lack positive retention','Full pairwise assembly, harness, service, load and thermal checks pending']),indent=2))
print('Native self-contained payload layout saved; bay edit moves cradle/deck; whole payload remains unverified')
