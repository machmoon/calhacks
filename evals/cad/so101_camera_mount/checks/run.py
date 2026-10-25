"""Run bounded verification only after the parent's actual GUI authoring execution."""
import argparse,hashlib,json,subprocess,sys,time
from pathlib import Path
HERE=Path(__file__).resolve().parent;ROOT=HERE.parents[3]
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def main():
 p=argparse.ArgumentParser();p.add_argument('--candidate',required=True);p.add_argument('--out',required=True);p.add_argument('--timeout',type=int,default=240);a=p.parse_args()
 if not 10<=a.timeout<=600:p.error('timeout must be10..600 seconds per process')
 candidate=Path(a.candidate).resolve();out=Path(a.out).resolve();out.mkdir(parents=True,exist_ok=False);started=time.time();contract=json.loads((HERE/'contract.json').read_text());source=ROOT/contract['source_model']
 result={'status':'running','candidate':str(candidate),'passed':False,'processes':[],'evaluator_hashes':{p.name:sha(p) for p in HERE.iterdir() if p.suffix in ['.py','.json']},'source_sha256':sha(source),'scope':'Static local camera-mount CAD verification; no whole-arm motion qualification'}
 def write():(out/'result.json').write_text(json.dumps(result,indent=2))
 if not candidate.is_file():result.update(status='failed',error='missing candidate FCStd');write();return 1
 before=sha(candidate);result['candidate_sha256']=before;write()
 try:
  for stage,path in [('baseline',candidate),('reopen',out/'checkpoint.FCStd')]:
   cmd=[str(ROOT/'cad/freecad/run.sh'),str(HERE/'probe.py'),'--candidate',str(path),'--stage',stage,'--output',str(out/(stage+'.json'))]
   if stage=='baseline':cmd+=['--save-copy',str(out/'checkpoint.FCStd')]
   t=time.time()
   with (out/(stage+'.log')).open('w') as log:process=subprocess.run(cmd,stdout=log,stderr=subprocess.STDOUT,timeout=a.timeout)
   result['processes'].append({'stage':stage,'returncode':process.returncode,'elapsed_seconds':time.time()-t});write()
   if process.returncode:raise RuntimeError(stage+' probe failed; see retained evidence')
  first=json.loads((out/'baseline.json').read_text());second=json.loads((out/'reopen.json').read_text())
  if first['status']!='complete' or second['status']!='complete' or [s['name'] for s in first['states']]!=['baseline'] or [s['name'] for s in second['states']]!=['reopened','thickness_edit','standoff_edit']:raise ValueError('incomplete native measurement states')
  result.update(status='complete',passed=first['passed'] and second['passed'],state_gates={s['name']:s['gates'] for s in first['states']+second['states']},limitations=second['limitations'])
 except Exception as error:result.update(status='failed',passed=False,error=str(error))
 result.update(candidate_unchanged=sha(candidate)==before,source_unchanged=sha(source)==result['source_sha256'],elapsed_seconds=time.time()-started);result['passed']=result['passed'] and result['candidate_unchanged'] and result['source_unchanged'];write();print(json.dumps(result,indent=2));return 0 if result['passed'] else 1
if __name__=='__main__':sys.exit(main())
