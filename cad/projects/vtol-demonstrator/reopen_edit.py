"""Fresh-process native parameter smoke; does not claim interface or flight validation."""
import FreeCAD as A,json,hashlib,argparse,time
from pathlib import Path
import importlib.util,sys
parser=argparse.ArgumentParser();parser.add_argument('--output-dir',required=True);args=parser.parse_args();out=Path(args.output_dir);source=out/'assembly.FCStd';before=hashlib.sha256(source.read_bytes()).hexdigest();metadata=out/('artifact-manifest.json' if (out/'artifact-manifest.json').exists() else 'progress.json');module_path=Path(json.loads(metadata.read_text())['proxy_module']);spec=importlib.util.spec_from_file_location(module_path.stem,module_path);module=importlib.util.module_from_spec(spec);sys.modules[module_path.stem]=module;spec.loader.exec_module(module);d=A.openDocument(str(source));p=d.DesignParameters
checks=[]
for key,value,name,metric,target in [('Span',1900,'RightOuterContour','YLength',950),('BoomHalfSpacing',340,'BoomRight','YMin',330),('BatteryLength',160,'BatteryReservation','XLength',160),('ServoLength',26,'ServoReservationRight','XLength',26)]:
 old=getattr(p,key).Value;setattr(p,key,value);start=time.time();d.recompute();obj=d.getObject(name);actual=getattr(obj.Shape.BoundBox,metric);checks.append(dict(parameter=key,value=value,object=name,metric=metric,actual=actual,expected=target,pass_=abs(actual-target)<.01,seconds=time.time()-start));setattr(p,key,old);d.recompute()
# Root chord changes actual source profile, rather than only metadata.
old=p.RootChord.Value;baseline=d.RightOuterContourSection0.Shape.BoundBox.XLength;p.RootChord=320;d.recompute();changed=d.RightOuterContourSection0.Shape.BoundBox.XLength;checks.append(dict(parameter='RootChord',value=320,baseline=baseline,actual=changed,pass_=changed>baseline+19.9));p.RootChord=old;d.recompute()
p.BoomHalfSpacing=340;d.recompute();
for side,sign in [('Right',1),('Left',-1)]:
 for end in ['Front','Rear']:
  n='LiftMountUpper'+side+end;o=d.getObject(n)
  if o:checks.append(dict(test='mount_spacing_dependency',object=n,actual=o.Placement.Base.y,expected=sign*340,pass_=abs(o.Placement.Base.y-sign*340)<1e-6))
d.saveAs(str(out/'edited-boom-spacing.FCStd'));A.closeDocument(d.Name);d=A.openDocument(str(out/'edited-boom-spacing.FCStd'));d.recompute();checks.append(dict(test='saved_reopened_edit',value=d.DesignParameters.BoomHalfSpacing.Value,right_boom_ymin=d.BoomRight.Shape.BoundBox.YMin,pass_=abs(d.BoomRight.Shape.BoundBox.YMin-330)<.01))
report=dict(source_unchanged=hashlib.sha256(source.read_bytes()).hexdigest()==before,checks=checks,scope='Parameter propagation smoke only; no fit/contact/load approval');(out/'reopen-edit.json').write_text(json.dumps(report,indent=2));print(json.dumps(report),flush=True)
