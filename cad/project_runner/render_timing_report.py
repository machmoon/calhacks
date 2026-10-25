"""Readable local report of observed timings; no model payloads embedded."""
import argparse
import html
import json
from pathlib import Path
from summarize_timing import summarize

def render(directory):
    root = Path(directory).resolve()
    data = summarize(root)
    def esc(value):
        return html.escape(str(value if value is not None else '—'))
    def table(rows, columns):
        return '<table><thead><tr>'+''.join('<th>'+esc(label)+'</th>' for _,label in columns)+'</tr></thead><tbody>'+''.join('<tr>'+''.join('<td>'+esc(round(row[key],2) if isinstance(row.get(key),float) else row.get(key))+'</td>' for key,_ in columns)+'</tr>' for row in rows)+'</tbody></table>'
    audit_path=root/'audit/result.json'
    audit=json.loads(audit_path.read_text()) if audit_path.is_file() else {}
    gate_rows=[{'gate':k,'status':v.get('status'),'reason':v.get('reason')} for k,v in audit.get('gates',{}).items()]
    sections = [('<h2>Top-level elapsed time</h2>', table(data['top_level_actions'], [('action','Action'),('duration_s','Seconds'),('status','Execution status')])),
                ('<h2>Every observed host action</h2>', table(data['all_observed_actions'], [('action','Action'),('label','Phase'),('duration_s','Seconds'),('status','Execution status')])),
                ('<h2>Native build boundaries</h2>', table(data['builder_phases'], [('phase','Phase'),('duration_s','Seconds'),('status','Observation')])),
                ('<h2>Independent probes</h2>', table(data['audit_probes'], [('stage','Probe'),('started_at','Started (UTC)'),('duration_s','Seconds'),('status','Execution status'),('engineering_passed','Probe predicate')])),
                ('<h2>Engineering gates</h2>', table(gate_rows, [('gate','Gate'),('status','Verdict'),('reason','Scope / remaining evidence')]))]
    pictures = ''.join(f'<figure><img src="cad/{name}.png" alt="{name} native CAD render"><figcaption>{name}</figcaption></figure>' for name in ['assembled','rear','cutaway','exploded'] if (root/'cad'/f'{name}.png').is_file())
    pictures += ''.join(f'<figure><img src="cad/{path.name}" alt="Live FreeCAD construction milestone"><figcaption>{path.stem}: actual live viewport</figcaption></figure>' for path in sorted((root/'cad').glob('live-*.png')))
    page = '<!doctype html><meta charset="utf-8"><title>Wright VTOL execution trace</title><style>body{font:16px system-ui;max-width:1120px;margin:40px auto;padding:0 24px;background:#f7f9fb;color:#17232f}h2{margin-top:36px}table{width:100%;border-collapse:collapse;background:white}td,th{text-align:left;padding:10px;border-bottom:1px solid #dce2e8;font-size:14px}th{background:#e8eef4}p{line-height:1.6}figure{display:inline-block;width:46%;margin:1%}img{width:100%}figcaption{color:#4b5d6b}code{overflow-wrap:anywhere}</style>'
    page += '<h1>Wright VTOL execution trace</h1><p><code>'+esc(data['run'])+'</code><br>Status: '+esc(data['execution_status'])+'<br>Observed wall time: '+esc(round(data['wall_elapsed_s'],2) if data['wall_elapsed_s'] is not None else None)+' seconds<br>First render published: '+esc(data.get('first_render_published_elapsed_s'))+' seconds after run start</p>'
    page += '<p>Nested actions overlap: do not add every row together. Model latency includes subscription transport; internal reasoning is unobservable. Execution completion does not establish aircraft qualification.</p>'
    ready_path=root/'cad-ready.json'
    if ready_path.is_file():
        ready=json.loads(ready_path.read_text())
        label='Live editable CAD ready' if ready.get('execution_kind')=='fixed_parameter_live_native_recipe' else 'Editable CAD and six views ready'
        scope='Engineering checks were explicitly not run. Zero model calls; supplied dimensions drive the prepared recipe.' if ready.get('engineering_checks')=='not_run' else 'Engineering verification was still pending at this milestone; full run time above includes subsequent checks.'
        page += '<h2>'+label+'</h2><p>'+esc(round(ready['elapsed_s'],2))+' seconds. '+scope+'</p>'
    comparison_path=root/'verification-comparison.json'
    if comparison_path.is_file():
        comparison=json.loads(comparison_path.read_text())
        page += '<h2>App verification runs</h2><p>'+esc(comparison['scope'])+'</p>'+table(comparison['runs'],[('run','Run'),('wall_seconds','Total seconds'),('first_render_seconds','First visible render, seconds'),('model_calls','Model calls')])
    access_path=root/'audit/access.json'
    if access_path.is_file():
        access=json.loads(access_path.read_text())
        if access.get('removal_sequence'):
            page += '<h2>Service sequence</h2><p>Tested driver paths: '+esc(len(access.get('checks',[])))+'. Local driver result: '+esc(access.get('driver_paths_passed'))+'. Full service qualification remains a separate gate.</p>'+table(access['removal_sequence'],[('part','Part'),('direction_mm','Removal displacement, mm'),('status','Path status'),('method','Evidence')])
        blocked=[{'fastener':row['fastener'],'obstacles':', '.join(hit['object'] for hit in row.get('clashes',[]))} for row in access.get('checks',[]) if row.get('clashes')]
        if blocked:
            page += '<h2>Blocked straight-driver paths</h2><p>These are modeled closed-assembly paths, not a validated disassembly sequence.</p>'+table(blocked,[('fastener','Fastener'),('obstacles','Blocking geometry')])
    sweep_path=root/'audit/sweeps.json'
    if sweep_path.is_file():
        sweeps=json.loads(sweep_path.read_text())
        overlaps=[{'propeller':row['propeller'],'obstacles':', '.join(hit['object'] for hit in row.get('clashes',[]))} for row in sweeps.get('sweeps',{}).get('checks',[]) if row.get('clashes')]
        if overlaps:
            page += '<h2>Inconclusive propeller envelopes</h2><p>Conservative envelopes overlap the following geometry. Exact rotating-part interfaces must resolve these observations; they are not silently ignored.</p>'+table(overlaps,[('propeller','Propeller'),('obstacles','Overlapping geometry')])
    benchmark_path=root/'render-benchmark/result.json'
    if benchmark_path.is_file():
        benchmark=json.loads(benchmark_path.read_text())
        page += '<h2>Controlled display comparison</h2><p>'+esc(benchmark.get('scope'))+' Maximum pixel-channel difference: '+esc(benchmark.get('max_pixel_channel_difference'))+'. Warm wall-time ratio: '+esc(round(benchmark.get('warm_wall_speedup',0),2))+'×.</p>'+table(benchmark.get('runs',[]),[('view','Renderer run'),('wall_s','Wall seconds'),('parent_cpu_s','Parent CPU seconds')])
    page += pictures + ''.join(title+body for title,body in sections)
    page += '<p><a href="performance-report.json">Machine-readable report</a> · <a href="timing-trace.jsonl">Raw host timing trace</a> · <a href="audit/result.json">Engineering evidence</a></p>'
    target=root/'performance-report.html';target.write_text(page);return target

if __name__ == '__main__':
    parser=argparse.ArgumentParser();parser.add_argument('run_dir');args=parser.parse_args();print(render(args.run_dir))
