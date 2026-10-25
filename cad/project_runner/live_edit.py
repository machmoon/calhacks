"""Submit explicit parameter edits to a running native GUI session."""
import json,os,time,uuid
from pathlib import Path
from timing import TimingTrace

def run(request,output):
 def field(key):
  values=[line.partition(':')[2].strip() for line in request.splitlines() if line.startswith(key+':')]
  if len(values)!=1:raise ValueError('Expected exactly one '+key)
  return values[0]
 import sys
 sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'freecad'))
 from live_edits import validate,write
 session=Path(field('WRIGHT_EDIT_SESSION')).resolve();values=validate(json.loads(field('WRIGHT_EDIT_JSON')))
 state=json.loads((session/'live-session.json').read_text())
 try:os.kill(state['pid'],0)
 except ProcessLookupError:raise RuntimeError('FreeCAD session is closed. Start a live demo and use its cad directory.')
 output=Path(output);output.mkdir(parents=True,exist_ok=True)
 if (output/'events.jsonl').exists():raise ValueError('Use a fresh output directory')
 trace=TimingTrace(output)
 def emit(kind,**data):
  with (output/'events.jsonl').open('a') as f:f.write(json.dumps(dict(type=kind,run=output.name,**trace.stamp(),**data))+'\n')
 identifier=uuid.uuid4().hex;queue=session/'edit-queue';pending=queue/(identifier+'.request.json');response=queue/(identifier+'.result.json');began=time.monotonic();status='failed'
 try:
  emit('run_started',domain='aircraft',request=request,execution_kind='persistent_native_parameter_edit')
  emit('agent_message',agent='FreeCAD',text='Editing the existing live document; one recompute, no model call or full rebuild.')
  write(pending,dict(parameters=values,expected_revision=state['revision']))
  while not response.exists():
   if time.monotonic()-began>120:raise TimeoutError('Edit result unknown after 120 seconds; inspect session before resubmitting')
   time.sleep(.1)
  result=json.loads(response.read_text());result['elapsed_s']=time.monotonic()-began
  (output/'result.json').write_text(json.dumps(result,indent=2))
  if result['status']=='failed':raise RuntimeError(result['error'])
  status=result['status'];emit('render',view='edited-native',path=result['image']);emit('artifact',path=result['native'],label='Edited native revision')
  emit('agent_message',agent='FreeCAD',text=f"Edit saved and displayed in {result['elapsed_s']:.2f}s. STEP and engineering checks were not updated.")
  emit('run_finished',passed=False,objective=result['elapsed_s'],objective_label='Native edit ready; checks not run')
 finally:trace.finish(status)
