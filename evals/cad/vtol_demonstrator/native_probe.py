"""Fresh-process native evidence; does not author or repair the demonstrator."""
import argparse,hashlib,json,sys,traceback,math,os,faulthandler
if os.environ.get("WRIGHT_CAD_DIAGNOSTICS")=="1":
 faulthandler.enable();faulthandler.dump_traceback_later(60,repeat=True)
from pathlib import Path
ROOT=Path(__file__).resolve().parents[3];sys.path.insert(0,str(ROOT/'cad/freecad'));sys.path.insert(0,str(ROOT/'cad/projects/vtol-demonstrator'))
import FreeCAD as App
import native,worker
from assembly_frames import world_shape
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
def value(x):return float(x.Value if hasattr(x,'Value') else x)
def bounds(o):
 b=native.shape_bounds(world_shape(o));return {key:float(getattr(b,key)) for key in ('XMin','XMax','YMin','YMax','ZMin','ZMax','XLength','YLength','ZLength')}
def valid(doc,names):
 rows=[]
 for name in names:
  o=doc.getObject(name)
  if o is None:rows.append({'name':name,'passed':False,'error':'missing object'});continue
  if not hasattr(o,'Shape'):rows.append({'name':name,'passed':False,'error':'no native Shape'});continue
  try:native.validate_live_feature(o)
  except Exception as error:
   rows.append({'name':name,'passed':False,'error':str(error),'cached_shape_valid':not o.Shape.isNull() and native.shape_is_valid(o.Shape)});continue
  rows.append({'name':name,'passed':not o.Shape.isNull() and native.shape_is_valid(o.Shape) and len(o.Shape.Solids)==1 and 'Invalid' not in o.State,'type':o.TypeId,'solids':len(o.Shape.Solids),'volume_mm3':o.Shape.Volume,'state':list(o.State)})
 return rows

def main(args):
 source=Path(args.native).resolve();out=Path(args.output).resolve();out.mkdir(parents=True,exist_ok=True)
 auditor_sources={str(p.relative_to(ROOT)):sha(p) for p in list(Path(__file__).parent.glob('*.py'))+[ROOT/'cad/freecad/assembly_frames.py',ROOT/'cad/freecad/native.py',ROOT/'cad/freecad/worker.py']}
 result={'auditor_sources':auditor_sources,'stage':args.stage,'native':str(source),'native_sha256':sha(source),'status':'running','freecad_version':App.Version()}
 path=out/(args.stage+'.json');path.write_text(json.dumps(result,indent=2))
 try:
  contract=json.loads(Path(args.contract).read_text());result['parent_runner_sha256']=contract.get('runner_sha256');physical=contract['physical_objects'];references=contract.get('reference_objects',[])
  if args.stage!='reopen' and contract.get('native_sha256') is not None and result['native_sha256']!=contract['native_sha256']:raise ValueError('Probe native hash differs from the frozen audit revision')
  if contract.get('proxy_module'):
   import importlib.util
   module=Path(contract['proxy_module']).resolve()
   if sha(module)!=contract['proxy_module_sha256']:raise ValueError('Declared proxy module hash mismatch')
   spec=importlib.util.spec_from_file_location(module.stem,module);loaded=importlib.util.module_from_spec(spec);sys.modules[module.stem]=loaded;spec.loader.exec_module(loaded)
   result['loaded_dependency']={'path':str(module),'sha256':sha(module)}
  doc=App.openDocument(str(source));worker._doc=doc;doc.recompute()
  if args.stage=='geometry':
   result.update(physical=valid(doc,physical),reference_objects=references,document_objects=len(doc.Objects),module_dependencies={str(p.relative_to(ROOT)):sha(p) for p in (ROOT/'cad/projects/vtol-demonstrator').glob('*.py')})
   result['passed']=bool(physical) and all(r['passed'] for r in result['physical'])
  elif args.stage=='pairs':
   collision=contract.get('collision_objects',physical)
   if len(collision)>512:raise ValueError('independent pair audit bounded to512 physicals; requires explicit reviewed larger job')
   result['check']=worker.check(collision,contacts=contract.get('contacts',[]),tolerance_mm=0.1,require_connected=False,summary=False,exhaustive_distances=False)
   result['coverage']={'objects':collision,'physical_count':len(physical),'candidate_hardware_count':len(set(collision)-set(physical)),'excluded_reference_objects':[n for n in references if n not in collision]}
   result['passed']=result['check']['passed']
  elif args.stage=='sweeps':
   import lift_sweep
   result['sweeps']=lift_sweep.check(doc,contract.get('collision_objects',physical));result['passed']=result['sweeps']['passed']
  elif args.stage=='access':
   import service_access
   result.update(service_access.check(doc,physical,contract.get('collision_objects',physical)))
  elif args.stage=='aero':
   import wing_analysis
   result['analysis']=wing_analysis.analyze(doc,str(out/'wing-analysis'),'RightOuterContour',velocity_m_s=18,alphas_deg=[0,4,8])
   result['passed']=result['analysis'].get('numerical_checks_passed') is True
  elif args.stage=='edits' or args.stage.startswith('edit_'):
   chosen=None if args.stage=='edits' else args.stage[5:]
   params=doc.getObject('DesignParameters');cases=[]
   if params is None:raise ValueError('DesignParameters missing')
   def test(name,changes,measure):
    if chosen is not None and chosen!=name:return
    old={k:value(getattr(params,k)) for k in changes}
    try:
     before=measure(old);old_hash=worker.geometry_fingerprint([doc.getObject('RightOuterContour')])
     for k,v in changes.items():setattr(params,k,v)
     doc.recompute();after=measure(changes);checks=after['checks'];v=valid(doc,physical)
     cases.append({'case':name,'changes':changes,'before':before,'after':after,'geometry_valid':all(r['passed'] for r in v),'passed':all(checks.values()) and all(r['passed'] for r in v),'physical_validation':v,'wing_changed':worker.geometry_fingerprint([doc.getObject('RightOuterContour')])!=old_hash})
    except Exception as error:cases.append({'case':name,'passed':False,'error':str(error)})
    finally:
     if chosen is None:
      for k,v in old.items():setattr(params,k,v)
      doc.recompute()
     result['cases']=cases;path.write_text(json.dumps(result,indent=2,allow_nan=False))
   def span(values):
    r=bounds(doc.RightOuterContour);l=bounds(doc.LeftOuterContour);target=values['Span']/2
    return {'right_ymax':r['YMax'],'left_ymin':l['YMin'],'expected_half_span':target,'checks':{'right_tip':abs(r['YMax']-target)<.01,'left_tip':abs(l['YMin']+target)<.01}}
   test('span',{'Span':value(params.Span)+100},span)
   section=doc.RightOuterContour.Sections[0] if hasattr(doc.RightOuterContour,'Sections') else None
   if section is None:cases.append({'case':'chord','passed':False,'status':'pending','reason':'No independently measurable native root section exposed'})
   else:
    original_chord=value(params.RootChord);original_extent=bounds(section)['XLength']
    def chord(values):
     extent=bounds(section)['XLength'];expected=original_extent*values['RootChord']/original_chord
     return {'root_section':section.Name,'extent_mm':extent,'expected_mm':expected,'checks':{'root_section_scale':abs(extent-expected)<.01}}
    test('chord',{'RootChord':original_chord+20},chord)
   def boom(values):
    coords={name:(bounds(doc.getObject(name))['YMin']+bounds(doc.getObject(name))['YMax'])/2 for name in ['BoomRight','BoomLeft']}
    return {'centers_y_mm':coords,'checks':{'right_center':abs(coords['BoomRight']-values['BoomHalfSpacing'])<.01,'left_center':abs(coords['BoomLeft']+values['BoomHalfSpacing'])<.01}}
   test('boom_spacing',{'BoomHalfSpacing':value(params.BoomHalfSpacing)+20},boom)
   def battery(values):
    b=bounds(doc.BatteryReservation);expected=[values[k] for k in ['BatteryLength','BatteryWidth','BatteryHeight']]
    return {'bounds':b,'representation':'nonphysical dimensional reservation','checks':{key:abs(b[key]-number)<.01 for key,number in zip(['XLength','YLength','ZLength'],expected)}}
   test('battery',{'BatteryLength':value(params.BatteryLength)+10,'BatteryWidth':value(params.BatteryWidth)+5,'BatteryHeight':value(params.BatteryHeight)+3},battery)
   def servo(values):
    measured={name:bounds(doc.getObject(name))['XLength'] for name in ['ServoReservationRight','ServoReservationLeft']}
    return {'lengths_mm':measured,'representation':'unresolved nonphysical servo reservation','checks':{name:abs(length-values['ServoLength'])<.01 for name,length in measured.items()}}
   test('servo_reservation',{'ServoLength':value(params.ServoLength)+4},servo)
   if chosen in (None,'servo_reservation'):cases.append({'case':'servo_hardware_fit','passed':False,'status':'pending','reason':'Servo model/interface not frozen; reservation is not verified servo geometry'})
   result.update(cases=cases,passed=bool(cases) and all(c['passed'] for c in cases))
   if chosen in (None,'span'):
    if chosen is None:params.Span=value(params.Span)+100;doc.recompute()
    edited=out/'edited.FCStd';doc.saveAs(str(edited));result.update(edited_native=str(edited),edited_sha256=sha(edited),saved_span=value(params.Span))
  elif args.stage=='reopen':
   params=doc.DesignParameters;before=value(params.Span);first=bounds(doc.RightOuterContour);params.Span=before+50;doc.recompute();after=bounds(doc.RightOuterContour);v=valid(doc,physical)
   result.update(loaded_span=before,new_span=value(params.Span),tip_before=first['YMax'],tip_after=after['YMax'],physical_validation=v,passed=abs(first['YMax']-before/2)<.01 and abs(after['YMax']-(before+50)/2)<.01 and all(r['passed'] for r in v))
  result['status']='complete';App.closeDocument(doc.Name)
 except Exception as error:result.update(status='failed',passed=False,error=str(error),traceback=traceback.format_exc())
 result['source_unchanged']=sha(source)==result['native_sha256'];path.write_text(json.dumps(result,indent=2,allow_nan=False));print(json.dumps({'status':result['status'],'passed':result.get('passed'),'output':str(path)}));return result
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('stage',choices=['geometry','pairs','edits','reopen','aero','access','sweeps','edit_span','edit_chord','edit_boom_spacing','edit_battery','edit_servo_reservation']);p.add_argument('native');p.add_argument('contract');p.add_argument('output');main(p.parse_args())
