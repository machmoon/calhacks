"""Content-free timing of observable host actions; never model reasoning."""
import atexit,json,re,time
from contextlib import contextmanager,nullcontext
from datetime import datetime,timezone
from pathlib import Path

class TimingTrace:
 def __init__(self,output):
  self.output=Path(output);self.origin=time.monotonic();self.rows=[];self.active={};self.closed=False;self.sequence=0
  self.path=self.output/'timing-trace.jsonl';self.summary=self.output/'timing-summary.json'
  atexit.register(self.finish,'interrupted')
 def stamp(self):return {'utc':datetime.now(timezone.utc).isoformat(timespec='milliseconds'),'monotonic_s':time.monotonic(),'elapsed_s':round(time.monotonic()-self.origin,6)}
 def event(self,kind,**fields):
  row=dict(type=kind,**self.stamp(),**fields)
  with self.path.open('a') as stream:stream.write(json.dumps(row)+'\n')
  return row
 def begin(self,name,label=None):
  self.sequence+=1;key=self.sequence
  # Labels are fixed builder phase descriptions, never request/response payloads.
  safe=None if label is None else re.sub(r'[^A-Za-z0-9 .,;:_()/+-]','?',str(label))[:160]
  if safe and any(token in safe.lower() for token in ['base64','bearer','sk-','token=']):safe='[redacted phase label]'
  row=self.event('action_started',action_id=key,action=name,label=safe);self.active[key]=(row,time.monotonic());return key
 def end(self,key,status='succeeded',error=None,return_code=None):
  start,tick=self.active.pop(key);row=self.event('action_finished',action_id=key,action=start['action'],label=start.get('label'),status=status,duration_s=round(time.monotonic()-tick,6),error_class=type(error).__name__ if error else None,return_code=return_code)
  self.rows.append(row)
 @contextmanager
 def span(self,name):
  key=self.begin(name);details={}
  try:yield details
  except BaseException as error:self.end(key,'failed',error);raise
  else:self.end(key,details.get('status','succeeded'),return_code=details.get('return_code'))
 def response(self,path):
  try:
   path=Path(path)
   if path.stat().st_size>8*1024*1024:raise ValueError('oversized response metadata')
   response=json.loads(path.read_text());usage={k:v for k,v in response.get('usage',{}).items() if k in {'input_tokens','output_tokens','cache_read_input_tokens','cache_creation_input_tokens'} and type(v) is int and v>=0}
   tools=[b['name'] for b in response.get('content',[]) if isinstance(b,dict) and b.get('type')=='tool_use' and isinstance(b.get('name'),str) and re.fullmatch(r'[A-Za-z0-9_-]{1,96}',b['name'])]
   self.event('model_response_observed',usage=usage,host_tool_names=tools,model='gpt-6-astra',reasoning_timing='unobservable',latency_scope='Aggregate external model runner, including subscription login status and codex exec; internal subcall times unavailable')
  except Exception as error:self.event('model_response_metadata_unavailable',error_class=type(error).__name__)
 def finish(self,status='completed'):
  if self.closed:return
  for key in list(self.active):self.end(key,'interrupted')
  self.event('timing_finished',status=status);self.closed=True
  self.summary.write_text(json.dumps({'schema_version':1,'status':status,**self.stamp(),'actions':self.rows,'scope':'Observed host actions only. Nested durations overlap and must not be summed as wall time. Builder phase boundaries are polled at approximately1second resolution. Model reasoning and internal Codex subcall timing are unobservable. No request/response bodies recorded.'},indent=2))

def span(trace,name):return trace.span(name) if trace else nullcontext({})
