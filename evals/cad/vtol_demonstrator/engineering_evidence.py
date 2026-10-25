"""Revision-bound bridge to Wright's existing aircraft checks and BOM inventory.

No catalog defaults are substituted for missing hardware or native mass data.
"""
import json, os, subprocess
from pathlib import Path
from report import sha

REQUIRED_BINDINGS=['native_sha256','bom_sha256','design_path','design_sha256','spec_path','spec_sha256','native_mass_reconciliation']

def collect(manifest, native_sha, package, root, output):
 package=Path(package);output=Path(output)
 bom_path=package/'bom.json'
 bom=json.loads(bom_path.read_text()) if bom_path.is_file() else {}
 items=bom.get('items',[]);objects=set(manifest.get('physical',[])+manifest.get('references',[]))
 rows=[]
 for item in items:
  # Only explicit instance mappings establish coverage; matching model names is
  # not proof of instance count or placement.
  mapping=manifest.get('bom_instances',{}).get(item['id'],[])
  count=item.get('quantity',0)
  rows.append({'id':item['id'],'quantity':count,'mapped_objects':mapping,'missing_objects':[n for n in mapping if n not in objects],
   'covered':len(mapping)==count and len(set(mapping))==count and all(n in objects for n in mapping),
   'mass_g_each':item.get('mass_g'),'hardware_released':item.get('compatibility_verified') is True})
 mass_known=sum(row['quantity']*row['mass_g_each'] for row in rows if isinstance(row['mass_g_each'],(int,float)))
 coverage={'rows':rows,'mapped_bom_rows':sum(r['covered'] for r in rows),'total_bom_rows':len(rows),'known_purchased_mass_subtotal_g':mass_known,'mass_scope':'Purchased BOM subtotal only; excludes unresolved masses and manufactured materials; not aircraft takeoff mass or CG'}
 descriptor=manifest.get('wright_air_binding')
 check={'status':'pending','execution_status':'not_run','required_binding_fields':REQUIRED_BINDINGS,'reason':'No revision-bound Wright-air design/spec and native mass reconciliation supplied. Catalog defaults cannot certify this native assembly.'}
 if descriptor:
  try:
   binding_path=Path(descriptor);binding_path=binding_path if binding_path.is_absolute() else package/binding_path;binding=json.loads(binding_path.read_text())
   missing=[key for key in REQUIRED_BINDINGS if key not in binding]
   if missing:raise ValueError('Missing binding fields: '+', '.join(missing))
   if binding['native_sha256']!=native_sha or binding['bom_sha256']!=sha(bom_path):raise ValueError('Native/BOM revision mismatch')
   paths={}
   for key in ['design','spec']:
    p=Path(binding[key+'_path']);p=p if p.is_absolute() else binding_path.parent/p
    if sha(p)!=binding[key+'_sha256']:raise ValueError(key+' input hash mismatch')
    paths[key]=p
   design=json.loads(paths['design'].read_text());parameters=manifest['design_parameters']
   for field,parameter in [('span_mm','Span'),('root_chord_mm','RootChord')]:
    if abs(design['wing'][field]-parameters[parameter])>1e-6:raise ValueError('Wright-air wing differs from native '+parameter)
   binary=Path(os.environ.get('WRIGHT_BINARY',str(Path(root)/'target/release/wright')))
   command=[str(binary),'aircraft','check',str(paths['design']),'--spec',str(paths['spec'])]
   with (output/'wright-air.log').open('w') as log:completed=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT,timeout=120)
   check.update(execution_status='complete',return_code=completed.returncode,local_checks_passed=completed.returncode==0,binding_sha256=sha(binding_path),reason='Wright-air checks executed on bound inputs. Native mass/material/CG, tail representation and hardware mapping reconciliation require independent review; successful analytical checks alone do not release the aircraft.')
   if completed.returncode:check['status']='failed'
  except Exception as error:check.update(status='inconclusive',execution_status='blocked',reason=str(error))
 evidence={'native_sha256':native_sha,'bom_sha256':sha(bom_path) if bom_path.is_file() else None,'bom_coverage':coverage,'wright_air':check}
 (output/'engineering-inputs.json').write_text(json.dumps(evidence,indent=2,allow_nan=False))
 return evidence
