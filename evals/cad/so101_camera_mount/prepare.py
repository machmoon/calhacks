"""Freeze a camera-mount request for Wright's existing subscription transport."""
import argparse,hashlib,json
from pathlib import Path

def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def prepare(native,interface,camera,out):
 native,interface,camera,out=map(lambda p:Path(p).resolve(),(native,interface,camera,out))
 if out.exists():raise ValueError('request destination exists')
 facts=json.loads(interface.read_text());spec=json.loads(camera.read_text())
 if facts.get('source_native_sha256')!=sha(native):raise ValueError('measured interface belongs to a different native source')
 prompt=Path(__file__).with_name('prompt.md').read_text()+'\nCAMERA\n'+json.dumps(spec,indent=2)+'\nWRIST INTERFACE\n'+json.dumps(facts,indent=2)+'\nINPUT FCSTD\n'+str(native)+'\nINPUT SHA256\n'+sha(native)
 request={'model':'gpt-6-astra','max_tokens':16000,'system':'You author editable native FreeCAD mechanical geometry. Submit one module with build(input_path,output_dir,checkpoint=None). Do not construct anything at module import. Preserve original source geometry.','messages':[{'role':'user','content':prompt}],'tools':[{'name':'submit_candidate','description':'Submit one native camera mount authoring module for host execution in real FreeCAD.','input_schema':{'type':'object','additionalProperties':False,'required':['python','summary'],'properties':{'python':{'type':'string'},'summary':{'type':'string'}}}}]}
 out.mkdir(parents=True);(out/'request.json').write_text(json.dumps(request,indent=2));(out/'prompt.txt').write_text(prompt)
 manifest={'native':str(native),'native_sha256':sha(native),'interface':str(interface),'interface_sha256':sha(interface),'camera':str(camera),'camera_sha256':sha(camera),'request_sha256':sha(out/'request.json'),'model':'gpt-6-astra','reasoning_effort':'low','execution_owner':'host real FreeCAD GUI; not this preparation script'}
 (out/'freeze.json').write_text(json.dumps(manifest,indent=2));return manifest
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('native');p.add_argument('interface');p.add_argument('camera');p.add_argument('output');a=p.parse_args();print(json.dumps(prepare(a.native,a.interface,a.camera,a.output),indent=2))
