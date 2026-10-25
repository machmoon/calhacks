"""Apply audited manufacturer CAD references without fabricating unavailable models."""
import json,hashlib,csv
from pathlib import Path
P=Path(__file__).resolve().parent
def rd(n):return json.loads((P/(n+'.json')).read_text())
def wr(n,d):(P/(n+'.json')).write_text(json.dumps(d,indent=2)+'\n')
a=rd('cad-acquisition');r=rd('holybro-geometry-inspection');m=rd('component-cad-manifest');src=rd('source-manifest')
map={'fc_current':'fc','gps_shared_full':'gps','pdb_top':'power_distribution'}
for d in a['downloads']:
 if d['status']!='downloaded':continue
 ident=map[d['id']];g=next(x for x in r['parts'] if x['id']==d['id'])
 m['components']=[x for x in m['components'] if x['bom_id']!=ident]
 m['components'].append({'bom_id':ident,'quantity':1,'geometry_class':'manufacturer_native_STEP_solids','path':str(P/g['path']),'sha256':g['sha256'],'source_id':'cad_'+d['id'],'native_valid':g['valid'],'solids':g['solids'],'faces':g['faces'],'coordinate_frame':'Unchanged vendor coordinates; see holybro-geometry-inspection.json','bounds_mm':g['bounds_mm'],'mounting_verified':False,'source_geometry_inspected':True,'keepout_status':'Native geometry excludes cable routing/service clearance; final installation pending','scope':{'fc_current':'Exact currentModelA casing geometry, not full electronic internals','gps_shared_full':'Manufacturer explicitly publishes sharedM8N/M9N/M10 CAD; legacyfilename does not assert M10 electronic internals','pdb_top':'Selected18138 top-entry STEP; bottom solder protrusions included'}[d['id']]})
 src['sources']=[x for x in src['sources'] if x['id']!='cad_'+d['id']];src['sources'].append(dict(d,id='cad_'+d['id'],primary=True,retrieved='2026-10-24'))
m['new_envelopes']=[x for x in m.get('new_envelopes',[]) if x['bom_id']!='power_distribution']
checks=[{'bom_id':'power','status':'no exact CAD located','examined':'Holybro official PM downloads and PM02V3product page','rejected_substitute':'pm02d-full_asm.stp is digitalPM02D, not analogPM02V3','available_dimensions_mm':[25,25,10],'mount_holes':'N/A in manufacturer comparison','next_geometry':'Dimensional reference with stock leads and capacitor separately; no invented board internals'}, {'bom_id':'esc','status':'official2Ddrawing retrieved; no exact STEP found','path':'sources/holybro-cad/tekko-drawing.pdf','scope':'Page2 annotated boardphoto32×17 vs overall spec34.3×17.3×4.5; preserve overallreservation; pad layout graphic is not dimensioned pad-center CAD'}, {'bom_id':'radio','status':'official100mW mechanical drawing retrieved; no exact STEP located','path':'sources/holybro-cad/sik100mw-drawing.jpg','rejected_substitute':'sik1W_case.stp belongs larger7–28V1W model, not selected5V100mW','drawing_mm':{'case':[47.3,27.8,10.4],'length_with_RPSMA':53.2,'antenna_stud_projection':5.9},'scope':'No mounting holes shown; retain cradle/strap and antenna keepout; datasheet28×53×10.7 differs slightly so use conservative53.2×28×10.7 reservation'}, {'bom_id':'rx','status':'official dimensioned product image and specs; no exactSTEP located','examined':'https://radiomasterrc.com/products/rp3-expresslrs-2-4ghz-nano-receiver','scope':'22×13×4mm body only; two65mmTantennas and pigtail separate'}, {'bom_id':'cruise_esc','status':'existing manufacturer specification envelope; no nativeCAD acquired','scope':'No fabricated electronic internals'}]
m['cad_search_results']=checks;m['evidence_history']='history/2026-10-24-before-cad-acquisition';wr('component-cad-manifest',m)
for d in a['drawings']:
 src['sources']=[x for x in src['sources'] if x['id']!='drawing_'+d['id']];src['sources'].append(dict(d,id='drawing_'+d['id'],primary=True,retrieved='2026-10-24'))
wr('source-manifest',src)
# Correct disproven PDB dimensions in active package; old snapshot is retained.
b=rd('bom');pdb=next(x for x in b['items'] if x['id']=='power_distribution');pdb['dimensions_mm']=[60,80,23.5];pdb['verified_facts']['mount_centers_mm']=[50,50];pdb['verified_facts']['native_hole_diameter_mm']=3.30000508;pdb['source_ids']=['pdb_top','cad_pdb_top','drawing_pdb-top-drawing.jpg'];pdb['verified_facts']['correction']='Earlier80x80x20/70pitch was side-entry drawing; replaced by actual selectedtop-entry evidence'
wr('bom',b)
with (P/'bom.csv').open('w',newline='') as f:
 w=csv.writer(f);w.writerow(['id','manufacturer','model','quantity','mass_g_each_unless_noted','dimensions_mm','status','unresolved','sources'])
 for x in b['items']:w.writerow([x['id'],x['manufacturer'],x['model'],x['quantity'],x['mass_g'],json.dumps(x['dimensions_mm']),x['status'],'; '.join(x['unresolved']),';'.join(x['source_ids'])])
i=rd('interface-table');i['additional_interfaces']['PDB']='TOP-entry18138 sourceCAD60x80x23approx;drawing23.5high;50x50pitchØ3.3bores. SourceXY90.00018/140.00028 and110.00022/160.00032,axisZ. PCBZ0..1.31760;bottomprotrusions−1.6824. Preserve insulation/standoffclearance.';wr('interface-table',i)
for f in P.glob('*.json'):json.loads(f.read_text())
h=rd('hashes');h['files']={str(f.relative_to(P)):hashlib.sha256(f.read_bytes()).hexdigest() for f in sorted(P.rglob('*')) if f.is_file() and f!=P/'hashes.json'};wr('hashes',h)
print('Manifest updated;3 STEP imports independently validated; history retained.')
