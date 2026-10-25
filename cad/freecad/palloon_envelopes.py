"""User-dimensioned component envelope library, not a resolved payload assembly."""
import hashlib,json,os
from pathlib import Path
import FreeCAD as App
import native,profiles,worker
ROOT=Path(__file__).resolve().parents[2]
source=ROOT/'cad/projects/agents_in_space/hardware-bom.json'
bom=json.loads(source.read_text())
out=Path(os.environ.get('WRIGHT_CAD_OUT',ROOT/'out/palloon-hardware-envelopes')).resolve();out.mkdir(parents=True,exist_ok=True)
worker.document('PalloonUserHardware');doc=worker.active()
physical=[];missing=[];mapping=[];x=0
for item in bom['items']:
    dims=item['dimensions_mm']
    if dims is None or None in dims:
        missing.append(dict(id=item['id'],quantity=item['quantity'],missing='Full 3D envelope and mounting/connector geometry'))
        continue
    for i in range(item['quantity']):
        name=item['id'].capitalize()+str(i+1)+'Envelope'
        native.make_part(doc,dict(name=name,kind='box',params=dict(zip(['x','y','z'],dims))))
        obj=doc.getObject(name);obj.Label=item['model']+' — reference envelope'
        obj.Placement.Base=App.Vector(x,0,0);physical.append(name)
        for prop,value in [('BOMItem',item['id']),('GeometryScope','User-supplied bounding envelope; no mounting holes or connector details'),('DimensionSource',item.get('dimension_reference',item['source']))]:
            obj.addProperty('App::PropertyString',prop,'Provenance');setattr(obj,prop,value)
        mapping.append(dict(object=name,bom_id=item['id'],instance=i+1,dimensions_mm=dims,catalogue_position_mm=[x,0,0]))
        x+=dims[0]+15
# Keep bare carrier footprint separate from the assembled kit envelope.
jetson=next(i for i in bom['items'] if i['id']=='jetson')
w,h,_=jetson['user_supplied_dimensions_mm'];sk=doc.addObject('Sketcher::SketchObject','JetsonFootprint')
sk.addProperty('App::PropertyBool','WrightProfile');sk.WrightProfile=True
profiles.assign(sk,[[0,70,0],[w,70,0],[w,70+h,0],[0,70+h,0]],native.vector)
sk.Label='Jetson bare carrier footprint — separate from assembled kit'
doc.recompute();native.validate_document(doc)
for m in mapping:
    o=doc.getObject(m['object']);b=o.Shape.BoundBox
    assert all(abs(a-c)<1e-7 for a,c in zip([b.XLength,b.YLength,b.ZLength],m['dimensions_mm']))
fit=worker.check(physical)
assert fit['passed'] and len(physical)==sum(i['quantity'] for i in bom['items'] if i['dimensions_mm'] is not None and None not in i['dimensions_mm'])
exports=worker.save(str(out),physical)
# Keep reference outline visible in native file, though absent from solid exchange.
sk.Visibility=True;doc.saveAs(exports['native'])
report=dict(scope='Dimensioned envelope catalogue; positions are display spacing, not payload packaging',source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),full_envelope_units=len(physical),bom_units=bom['total_units'],mapping=mapping,unmodeled_units=missing,reference_profiles=['JetsonFootprint'],fit=fit,exports=exports,payload_complete=False,manufacturer_verified_envelope_ids=[i['id'] for i in bom['items'] if i['manufacturer_dimensions_verified']],mass_properties_valid=False)
(out/'result.json').write_text(json.dumps(report,indent=2))
print(json.dumps({k:report[k] for k in ['full_envelope_units','bom_units','payload_complete']}))
