"""Probe the pinned upstream Rocket Workbench importer in native FreeCAD.

This reports import health only. Import success never means source component
coverage, placement, manufacturing readiness or flight approval has passed.
"""
import json
import argparse
import hashlib
import difflib
import math
import importlib
import os
from pathlib import Path
import subprocess
import sys
import traceback
import zipfile
import xml.etree.ElementTree as ET
import FreeCAD as App
from exchange import export_step,export_glb

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--mutate-tube-length',action='store_true',help='Negative control: increase one imported inner tube length by 1 mm')
parser.add_argument('--mutate-rail-spacing',action='store_true',help='Negative control: increase one rail-button pitch by 1 mm')
args=parser.parse_args()
if args.mutate_tube_length and args.mutate_rail_spacing:parser.error('choose one negative control')
workbench=Path(os.environ['WRIGHT_ROCKET_WORKBENCH']).resolve()
sys.path.insert(0,str(workbench))
root=Path(__file__).resolve().parents[2]
out=Path(os.environ['WRIGHT_CAD_OUT']).resolve()
out.mkdir(parents=True,exist_ok=True)
patch=subprocess.check_output(['git','-C',str(workbench),'diff','--binary'])
untracked=subprocess.check_output(['git','-C',str(workbench),'ls-files','--others','--exclude-standard'],text=True).splitlines()
for relative in untracked:
    path=workbench/relative
    if path.suffix=='.py':
        patch+=('diff --git a/'+relative+' b/'+relative+'\nnew file mode 100644\n').encode()
        patch+=''.join(difflib.unified_diff([],path.read_text().splitlines(keepends=True),fromfile='/dev/null',tofile='b/'+relative)).encode()
(out/'workbench.patch').write_bytes(patch)
report=dict(workbench_python_sha256={str(p.relative_to(workbench)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(workbench.rglob('*.py'))},exchange_sha256=hashlib.sha256((root/'cad/freecad/exchange.py').read_bytes()).hexdigest(),evaluator_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),negative_control=args.mutate_tube_length or args.mutate_rail_spacing,workbench_patch_sha256=hashlib.sha256(patch).hexdigest() if patch else None,
            workbench_modified=bool(patch),workbench_url='https://github.com/davesrocketshop/Rocket',
            workbench_commit=subprocess.check_output(['git','-C',str(workbench),'rev-parse','HEAD'],text=True).strip(),
            freecad_version=App.Version(),full_cad_benchmark_passed=False,imports=[])
def inventory(path):
    with zipfile.ZipFile(path) as archive:
        tree=ET.fromstring(archive.read('rocket.ork'))
    entries=[]
    def visit(element,path,parent_component_path=None):
        for index,child in enumerate(element):
            child_path=f'{path}/{child.tag}[{index}]'
            if element.tag=='subcomponents':
                entries.append(dict(path=child_path,tag=child.tag,name=child.findtext('name'),parent_component_path=parent_component_path,
                                    source_id=child.findtext('id'),fin_count=child.findtext('fincount'),
                                    source_values={e.tag:e.text for e in child if len(e)==0},
                                    source_attributes={e.tag:e.attrib for e in child if len(e)==0},
                                    status='not_visited'))
            visit(child,child_path,child_path if element.tag=='subcomponents' else parent_component_path)
    visit(tree,'/openrocket')
    return entries

sax_module=importlib.import_module('Rocket.Importer.OpenRocket.SaxElement')
original_create=sax_module.Element.createChild
coverage=[];features={}
def tracked_create(self,tag,attributes,filename,line):
    item=None
    if self._tag=='subcomponents':
        item=next((e for e in coverage if e['tag']==tag and e['status']=='not_visited'),None)
        if item is None:raise ValueError('component event has no source inventory entry: '+tag)
        item['line']=line
    try:
        child=original_create(self,tag,attributes,filename,line)
        if item is not None:
            item['handler']=type(child).__name__
            item['status']='ignored' if isinstance(child,sax_module.NullElement) else 'created'
            feature=getattr(child,'_feature',None)
            obj=getattr(feature,'_obj',None)
            if obj is not None:
                item['native_object']=obj.Name;features[item['path']]=obj
        return child
    except Exception as error:
        if item is not None:item.update(status='failed',error=str(error))
        raise
sax_module.Element.createChild=tracked_create

for source in json.loads((root/'evals/cad/references/sources.json').read_text()):
    source_path=root/'evals/cad/references'/source['file']
    if hashlib.sha256(source_path.read_bytes()).hexdigest()!=source['sha256']:raise ValueError('reference file hash mismatch')
    coverage=inventory(source_path);features={}
    entry=dict(source=source,import_passed=False,component_coverage=coverage,
               component_coverage_passed=False,geometry_scope='Importer health and source definition coverage only; instance placement and manufacturing completeness unverified')
    try:
        import importORK
        diagnostics=[]
        importer_module=importlib.import_module('Rocket.Importer.OpenRocket.OpenRocket')
        importer_module._err=lambda message: diagnostics.append(str(message)+'\n'+traceback.format_exc())
        entry['importer_diagnostics']=diagnostics
        doc=importORK.open(str(root/'evals/cad/references'/source['file']))
        if args.mutate_tube_length or args.mutate_rail_spacing:
            tag='railbutton' if args.mutate_rail_spacing else 'innertube'
            prop='InstanceSeparation' if args.mutate_rail_spacing else 'Length'
            candidate=next((e for e in coverage if e['tag']==tag and e['path'] in features),None)
            if candidate is not None:
                obj=features[candidate['path']]
                previous=getattr(obj,prop).Value
                setattr(obj,prop,previous+1)
                entry['negative_control']=dict(component=candidate['path'],property=prop,before_mm=previous,after_mm=getattr(obj,prop).Value)
        doc.recompute()
        shapes=[]
        errors=[]
        for obj in doc.Objects:
            if 'Invalid' in obj.State:errors.append(dict(object=obj.Name,state=list(obj.State)))
            if hasattr(obj,'Shape') and not obj.Shape.isNull():
                shapes.append(dict(name=obj.Name,type=obj.TypeId,valid=obj.Shape.isValid(),solids=len(obj.Shape.Solids),volume_mm3=obj.Shape.Volume))
        entry.update(objects=len(doc.Objects),shapes=shapes,invalid_objects=errors)
        for item in coverage:
            obj=features.get(item['path'])
            if obj is not None and hasattr(obj,'Shape'):
                item['shape_valid']=not obj.Shape.isNull() and obj.Shape.isValid()
                item['solid_count']=len(obj.Shape.Solids)
                item['native_volume_mm3']=obj.Shape.Volume
                values=item['source_values']
                checks=[]
                if item.get('fin_count'):
                    checks.append(dict(check='fin instance count',expected=int(item['fin_count']),actual=len(obj.Shape.Solids),passed=len(obj.Shape.Solids)==int(item['fin_count'])))
                if item['tag'] in ['bodytube','innertube','tubecoupler','launchlug']:
                    try:
                        length=float(values['length'])*1000
                        radius=float(values.get('radius',values.get('outerradius')))*1000
                        wall=float(values['thickness'])*1000
                        count=int(values.get('instancecount','1'))
                        expected=math.pi*(radius**2-max(0,radius-wall)**2)*length*count
                        checks.append(dict(check='analytic tube material volume',expected_mm3=expected,actual_mm3=obj.Shape.Volume,passed=abs(obj.Shape.Volume-expected)<=max(1e-5,expected*1e-7)))
                    except (KeyError,ValueError,TypeError):
                        item['analytic_volume_status']='not evaluated: automatic or unavailable source dimensions'
                if item['tag']=='railbutton':
                    count=int(values['instancecount']);od=float(values['outerdiameter'])*1000;inner=float(values['innerdiameter'])*1000
                    height=float(values['height'])*1000;base=float(values['baseheight'])*1000;flange=float(values['flangeheight'])*1000
                    expected=math.pi/4*(od**2*(base+flange)+inner**2*(height-base-flange))*count
                    checks.append(dict(check='rail button envelope volume',expected_mm3=expected,actual_mm3=obj.Shape.Volume,passed=abs(obj.Shape.Volume-expected)<1e-5))
                    checks.append(dict(check='rail button instance count',expected=count,actual=len(obj.Shape.Solids),passed=len(obj.Shape.Solids)==count))
                    centers=sorted(s.CenterOfMass.x for s in obj.Shape.Solids)
                    pitch=float(values['instanceseparation'])*1000
                    checks.append(dict(check='rail button axial pitch',expected_mm=pitch,actual_mm=[b-a for a,b in zip(centers,centers[1:])],passed=len(centers)==count and all(abs(b-a-pitch)<1e-5 for a,b in zip(centers,centers[1:]))))
                    item['geometry_scope']='Source solid spool envelope; source gives no screw bore or installation fastener details'
                parent=next((e for e in coverage if e['path']==item['parent_component_path']),None)
                parent_obj=features.get(item['parent_component_path'])
                if parent is not None and parent_obj is not None and item['source_attributes'].get('axialoffset',{}).get('method')=='bottom':
                    try:
                        expected=(float(parent['source_values']['length'])-float(values['length'])+float(values['axialoffset']))*1000
                        parent_location=parent_obj.getGlobalPlacement()
                        actual=parent_location.Rotation.inverted().multVec(obj.getGlobalPlacement().Base-parent_location.Base).x
                        checks.append(dict(check='bottom-referenced axial placement',expected_mm=expected,actual_mm=actual,passed=abs(actual-expected)<1e-5))
                    except (KeyError,ValueError,TypeError):
                        item['axial_placement_status']='not evaluated: unavailable source dimensions'
                if item['tag']=='railbutton' and parent_obj is not None:
                    # Source base-center reference mapped from OR Y-radial to
                    # the workbench's Z-radial frame (x,y,z)->(x,-z,y).
                    native_parent=parent_obj.getGlobalPlacement()
                    centers=[native_parent.Rotation.inverted().multVec(s.CenterOfMass-native_parent.Base) for s in obj.Shape.Solids]
                    centers.sort(key=lambda v:v.x)
                    angle=math.radians(float(values['angleoffset']))
                    radius=parent_obj.Diameter.Value/2
                    neck=height-base-flange
                    zbar=(od**2*base*(base/2)+inner**2*neck*(base+neck/2)+od**2*flange*(height-flange/2))/(od**2*(base+flange)+inner**2*neck)
                    expected_radial=[-math.sin(angle)*(radius+zbar),math.cos(angle)*(radius+zbar)]
                    radial_error=max(max(abs(c.y-expected_radial[0]),abs(c.z-expected_radial[1])) for c in centers)
                    checks.append(dict(check='rail button radial placement against parent surface',max_error_mm=radial_error,passed=radial_error<1e-5))
                    method=item['source_attributes']['axialoffset']['method']
                    if method in ['top','middle','bottom']:
                        parent_length=float(parent['source_values']['length'])*1000
                        first=float(values['axialoffset'])*1000+{'top':0,'middle':parent_length/2,'bottom':parent_length}[method]
                        error=max(abs(c.x-(first+i*pitch)) for i,c in enumerate(centers))
                        checks.append(dict(check='rail button source-referenced axial centers',reference=method,max_error_mm=error,passed=error<1e-5))
                item['independent_checks']=checks

            if item['status']=='created' and item['tag']!='stage' and not item.get('shape_valid',False):
                item['status']='created_without_valid_shape'
        entry['component_coverage_passed']=all(e['status']=='created' for e in coverage)
        checks=[c for e in coverage for c in e.get('independent_checks',[])]
        entry['independent_checks_passed']=all(c['passed'] for c in checks) if checks else None
        entry['independent_checks_count']=sum(len(e.get('independent_checks',[])) for e in coverage)
        entry['coverage_counts']={status:sum(e['status']==status for e in coverage) for status in sorted({e['status'] for e in coverage})}

        if diagnostics or not shapes or errors or any(not s['valid'] for s in shapes):raise ValueError(f'invalid import: {errors}')
        folder=out/Path(source['file']).stem
        folder.mkdir(exist_ok=True)
        doc.saveAs(str(folder/'imported.FCStd'))
        entry.update(import_passed=True,objects=len(doc.Objects),shapes=shapes,native_path=str(folder/'imported.FCStd'))
        entry['materials_verified']=False
        if not (args.mutate_tube_length or args.mutate_rail_spacing):
            physical=[features[e['path']] for e in coverage if e['status']=='created' and e['tag']!='stage' and e.get('shape_valid')]
            try:
                entry['exchange']=dict(scope='Only supported source components; recovery omissions remain explicit',
                    component_definitions=len(physical),physical_solids=sum(len(o.Shape.Solids) for o in physical),
                    step_schema=export_step(physical,folder/'partial.step'),
                    glb=export_glb(physical,folder/'partial.glb'),passed=True)
            except Exception:
                entry['exchange']=dict(passed=False,error=traceback.format_exc())

    except Exception:
        entry['error']=traceback.format_exc()
    finally:
        for name in list(App.listDocuments()):App.closeDocument(name)
    report['imports'].append(entry)
    (out/'result.json').write_text(json.dumps(report,indent=2))
print(json.dumps(report,indent=2))
