"""Rejected R8 arch must fail even when its metadata claims inverted-V."""
import argparse,json
from pathlib import Path
from layout_acceptance import evaluate,sha,TAIL,section,world_shape
import FreeCAD as App

def run(source,manifest,output):
 source,manifest,output=map(lambda p:Path(p).resolve(),[source,manifest,output]);output.mkdir(parents=True,exist_ok=True);before=sha(source)
 m=json.loads(manifest.read_text());m['tail_configuration']='inverted_v';m['tail_objects']=TAIL
 # Deliberately forge the declared output names too; native geometry is untouched.
 mapping={'FinRight':'TailSurfaceRight','FinLeft':'TailSurfaceLeft','RudderRight':'RuddervatorRight','RudderLeft':'RuddervatorLeft'}
 m['physical']=[mapping.get(n,n) for n in m['physical'] if n not in ['Stabilizer','Elevator']]
 m['collision_objects']=[mapping.get(n,n) for n in m['collision_objects'] if n not in ['Stabilizer','Elevator']]
 forged=output/'forged-r8-manifest.json';forged.write_text(json.dumps(m,indent=2))
 result=evaluate(source,forged,output/'rejected-r8.json')
 assert not result['passed'] and result['source_unchanged'] and sha(source)==before
 assert 'Missing native object' in result.get('error',''),result
 doc=App.openDocument(str(source));doc.recompute()
 center_sections=[section(world_shape(doc.getObject('Stabilizer')),y) for y in [16,3.2]]
 App.closeDocument(doc.Name)
 assert abs(center_sections[1]['z_mid_mm']-center_sections[0]['z_mid_mm'])<1e-5,'Expected native R8 center roof is not flat'
 summary={'native_center_roof_sections':center_sections,'geometry_negative':'Actual R8 central sections have zero inward rise; fail R9 inclined-half criterion even independent of object names','passed':True,'control':'R8 native arch with forged inverted-V metadata and declared names rejected','native_sha256':before,'source_unchanged':True,'failure':result['error']};(output/'result.json').write_text(json.dumps(summary,indent=2));print(json.dumps(summary))
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('native');p.add_argument('manifest');p.add_argument('output');a=p.parse_args();run(a.native,a.manifest,a.output)
