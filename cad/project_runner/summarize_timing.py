"""Bounded report of observable execution, without payloads or reasoning claims."""
import argparse,json,math,time,re
from datetime import datetime,timezone
from pathlib import Path
LIMIT=16*1024*1024

def number(value):return float(value) if type(value) in (int,float) and math.isfinite(value) and value>=0 else None
def read_json(path):
 if not path.is_file():return {}
 if path.stat().st_size>LIMIT:raise ValueError('Timing report input exceeds16MiB: '+path.name)
 return json.loads(path.read_text())
def read_lines(path):
 if not path.is_file():return [],0
 if path.stat().st_size>LIMIT:raise ValueError('Timing trace exceeds16MiB: '+path.name)
 rows=[];invalid=0
 for line in path.read_text().splitlines():
  if len(line)>128*1024:raise ValueError('Oversized timing record')
  try:row=json.loads(line)
  except json.JSONDecodeError:invalid+=1;continue
  if isinstance(row,dict):rows.append(row)
  else:invalid+=1
  if len(rows)>10000:raise ValueError('Timing record count exceeds10000')
 return rows,invalid

def union_duration(intervals):
 total=0;last=None
 for start,end in sorted(intervals):
  if last is None:last=[start,end]
  elif start<=last[1]:last[1]=max(last[1],end)
  else:total+=last[1]-last[0];last=[start,end]
 return total+(last[1]-last[0] if last else 0)

def summarize(directory,now_monotonic=None):
 root=Path(directory).resolve();now=time.monotonic() if now_monotonic is None else now_monotonic
 trace,invalid=read_lines(root/'timing-trace.jsonl');phases,bad_phase=read_lines(root/'cad/build-phases.jsonl');probes,bad_probe=read_lines(root/'audit/probe-timings.jsonl')
 starts={r['action_id']:r for r in trace if r.get('type')=='action_started' and 'action_id'in r};ends={r['action_id']:r for r in trace if r.get('type')=='action_finished' and 'action_id'in r};actions=[]
 for key,start in starts.items():
  begin=number(start.get('monotonic_s'));end=ends.get(key);stop=number(end.get('monotonic_s')) if end else now
  if begin is None or stop is None or stop<begin:continue
  actions.append({'id':key,'action':str(start.get('action','unknown'))[:96],'label':start.get('label'),'started_at':start.get('utc'),'finished_at':end.get('utc') if end else None,'status':end.get('status') if end else 'unfinished_observation','duration_s':round(stop-begin,6),'error_class':end.get('error_class') if end else None,'return_code':end.get('return_code') if end else None,'start_monotonic_s':begin,'end_monotonic_s':stop})
 top=[]
 for row in actions:
  nested=any(other['id']!=row['id'] and other['start_monotonic_s']<=row['start_monotonic_s'] and other['end_monotonic_s']>=row['end_monotonic_s'] and (other['start_monotonic_s']<row['start_monotonic_s'] or other['id']<row['id']) for other in actions)
  if not nested:top.append(row)
 finish=next((r for r in reversed(trace) if r.get('type')=='timing_finished'),None)
 origin=next((r['monotonic_s']-r['elapsed_s'] for r in trace if number(r.get('monotonic_s')) is not None and number(r.get('elapsed_s')) is not None),None)
 wall=number(finish.get('elapsed_s')) if finish else max(0,now-origin) if origin is not None else None
 build=next((r for r in actions if r['action']=='freecad_build'),None);builder=[]
 for index,phase in enumerate(phases):
  begin=number(phase.get('monotonic_seconds'));stop=number(phases[index+1].get('monotonic_seconds')) if index+1<len(phases) else build['end_monotonic_s'] if build else now
  if begin is None or stop is None or stop<begin:continue
  builder.append({'phase':str(phase.get('phase','unknown'))[:200],'started_unix_seconds':phase.get('unix_seconds'),'duration_s':round(stop-begin,6),'status':'boundary_observed' if index+1<len(phases) else 'completed_interval' if build and build['status']!='unfinished_observation' else 'unfinished_observation'})
 latest={}
 for row in probes:
  stage=row.get('stage')
  if isinstance(stage,str):latest[stage]=row
 audit=[]
 for stage,row in latest.items():
  duration=number(row.get('duration_seconds'));begin=number(row.get('started_monotonic_seconds'))
  if duration is None and begin is not None:duration=max(0,now-begin)
  audit.append({'stage':stage[:96],'status':row.get('status'),'duration_s':duration,'started_at':row.get('started_at'),'finished_at':row.get('finished_at'),'engineering_passed':row.get('engineering_passed'),'process_returncode':row.get('process_returncode'),'error_class':row.get('error',{}).get('type') if isinstance(row.get('error'),dict) else None})
 responses=[]
 for row in trace:
  if row.get('type')=='model_response_observed':responses.append({'usage':{k:v for k,v in row.get('usage',{}).items() if k in {'input_tokens','output_tokens','cache_read_input_tokens','cache_creation_input_tokens'} and type(v) is int and v>=0},'host_tool_names':[v for v in row.get('host_tool_names',[]) if isinstance(v,str) and re.fullmatch(r'[A-Za-z0-9_-]{1,96}',v)][:16],'model':'gpt-6-astra','reasoning_timing':'unobservable'})
 events,bad_events=read_lines(root/'events.jsonl')
 render_publications=[{'view':str(row.get('view','unknown'))[:96],'elapsed_s':number(row.get('elapsed_s')),'utc':row.get('utc')} for row in events if row.get('type')=='render']
 report=read_json(root/'audit/result.json');manifest=read_json(root/'cad/artifact-manifest.json');request=read_json(root/'request.json')
 intervals=[(r['start_monotonic_s'],r['end_monotonic_s']) for r in top];covered=union_duration(intervals)
 result={'schema_version':1,'generated_at':datetime.now(timezone.utc).isoformat(timespec='milliseconds'),'run':root.name,'execution_status':finish.get('status') if finish else request.get('status','unknown'),'wall_elapsed_s':wall,'observed_top_level_union_s':round(covered,6),'unattributed_wall_s':max(0,wall-covered) if wall is not None else None,'top_level_actions':top,'all_observed_actions':actions,'unfinished_actions':[r for r in actions if r['status']=='unfinished_observation'],'builder_phases':builder,'render_publications':render_publications,'first_render_published_elapsed_s':next((r['elapsed_s'] for r in render_publications if r['elapsed_s'] is not None),None),'audit_probes':audit,'model_responses':responses,'gate_statuses':{k:v.get('status') for k,v in report.get('gates',{}).items()},'render_failures':[{'view':key,'status':'failed','detail_source':'cad/artifact-manifest.json'} for key in manifest.get('render_failures',{})],'ignored_incomplete_or_invalid_jsonl_records':invalid+bad_phase+bad_probe+bad_events,'limitations':['Observable host actions only; model reasoning and internal login/codex exec timings are not available.','Nested phase/probe durations overlap parent stages. Totals use the union of top-level intervals, never a sum of all rows.','Unfinished durations extend to this snapshot, not proof the external process remains alive.','Builder boundaries use native phase markers; polled parent observations have approximately1second resolution.','Payloads, commands, error bodies and image/base64 content are omitted.']}
 temporary=root/'performance-report.json.tmp';temporary.write_text(json.dumps(result,indent=2,allow_nan=False));temporary.replace(root/'performance-report.json');return result
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('run_dir');a=p.parse_args();r=summarize(a.run_dir);print(json.dumps({'report':str(Path(a.run_dir).resolve()/'performance-report.json'),'execution_status':r['execution_status'],'wall_elapsed_s':r['wall_elapsed_s'],'unfinished_actions':[x['action'] for x in r['unfinished_actions']]}))
