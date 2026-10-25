"""Native proposed VTOL layout. Run with cad/freecad/run.sh; no complete-aircraft claim."""
import argparse,json,hashlib,time,math,sys
from pathlib import Path
import FreeCAD as A,Part
import vtol_features,surface_preview
ROOT=Path(__file__).resolve().parents[3]
a=argparse.ArgumentParser();a.add_argument('--output-dir',required=True);a.add_argument('--package');a.add_argument('--design');a=a.parse_args()
out=Path(a.output_dir).resolve();out.mkdir(parents=True,exist_ok=True);start=time.time()
manifest={'schema_version':1,'design_revision':'layout-r2-saddles','status':'building','complete_vtol':False,'hardware_ready':False,'physical':[],'references':[],'helpers':[],'unresolved':['Hardware package is not frozen; component geometry does not establish compatibility.','Hardware-specific mounts, screws and installation paths pending verified component interfaces.','Structure and joints are not sized for loads; contact is not strength.','Cavity uses reduced section thickness, not certified constant wall.','Control hinges, linkages and servo travel not verified.','Mass/CG, wiring pinout, thermal design, propulsion and endurance remain pending.','All-pair clearance and connectivity audit separate; no collision-free assertion.'],'package':a.package}
manifest['generator_sources']={str(path):hashlib.sha256(path.read_bytes()).hexdigest() for path in [Path(__file__).resolve(),Path(vtol_features.__file__).resolve()]}
if a.package:
 package=Path(a.package)
 if not package.is_dir():raise ValueError('--package must be a package directory')
 manifest['package_sources']={path.name:hashlib.sha256(path.read_bytes()).hexdigest() for path in sorted(package.glob('*.json'))}
manifest['images']={n:str(out/(n+'.png')) for n in ['assembled','internals','cutaway','exploded']}
def phase(s):
 manifest['status']=s;manifest['elapsed_seconds']=time.time()-start;(out/'progress.json').write_text(json.dumps(manifest,indent=2));print(s,flush=True)
def fail(typ,error,tb):
 manifest['status']='failed';manifest['error']=str(error);(out/'failure.json').write_text(json.dumps(manifest,indent=2));sys.__excepthook__(typ,error,tb)
sys.excepthook=fail
defaults=dict(Span=1800,RootChord=300,BoomHalfSpacing=320,BatteryLength=152,BatteryWidth=56,BatteryHeight=43,ServoLength=24,MotorFrontX=120,MotorRearX=700)
ranges=dict(Span=(1600,2000),RootChord=(280,340),BoomHalfSpacing=(280,380),BatteryLength=(147,165),BatteryWidth=(51,65),BatteryHeight=(38,50),ServoLength=(20,32),MotorFrontX=(80,140),MotorRearX=(680,740))
if a.design:
 supplied=json.loads(Path(a.design).read_text())
 if not isinstance(supplied,dict):raise ValueError('Design must be a JSON object')
 for key,value in supplied.items():
  if key not in defaults or isinstance(value,bool) or not isinstance(value,(int,float)) or not math.isfinite(value) or not ranges[key][0]<=value<=ranges[key][1]:raise ValueError('Unsupported/out-of-range design parameter: '+key)
 defaults.update(supplied)
manifest['design_parameters_mm']=defaults;manifest['input_bounds_not_engineering_validation']=ranges
d=A.newDocument('VTOLDemonstrator');p=d.addObject('App::FeaturePython','DesignParameters')
for n,v in defaults.items():p.addProperty('App::PropertyLength',n,'Design');setattr(p,n,v)
p.addProperty('App::PropertyString','Status');p.Status='Provisional layout; hardware and engineering checks incomplete'
def classify(o,role,kind='physical'):
 o.addProperty('App::PropertyString','Fidelity');o.Fidelity=role;manifest[kind].append(o.Name);return o
def box(n,l,w,h,pos,kind='helpers'):
 o=d.addObject('Part::Box',n);o.Length=l;o.Width=w;o.Height=h;o.Placement.Base=A.Vector(*pos);manifest[kind].append(n);return o
def cyl(n,r,h,pos,axis=(0,0,1),kind='helpers'):
 o=d.addObject('Part::Cylinder',n);o.Radius=r;o.Height=h;o.Placement=A.Placement(A.Vector(*pos),A.Rotation(A.Vector(0,0,1),A.Vector(*axis)));manifest[kind].append(n);return o
def boolean(n,typ,base,tool,physical=True):
 o=d.addObject('Part::'+typ,n);o.Base=base;o.Tool=tool;o.Refine=True
 if physical:classify(o,'Original provisional airframe geometry')
 else:manifest['helpers'].append(n)
 return o
def mirror(n,o,kind='physical'):
 m=d.addObject('Part::Mirroring',n);m.Source=o;m.Normal=A.Vector(0,1,0);m.Base=A.Vector(0,0,0);manifest[kind].append(n);return m
phase('Building smooth parametric S3024 wing sections and native lofts')
source=ROOT/'cad/projects/tsung_vtol/references/s3024.dat';coords=[]
for line in source.read_text().splitlines()[1:]:
 try:x,z=map(float,line.split());coords.append([x,z])
 except ValueError:pass
manifest['airfoil_source']={'path':str(source),'sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'name':'S3024'}
def loft(n,kind,etas,inner=False):
 sections=[]
 for i,eta in enumerate(etas):
  o=d.addObject('Part::FeaturePython',n+'Section'+str(i));vtol_features.Section(o,kind);o.Parameters=p;o.Eta=eta;o.Inner=inner;o.Coordinates=json.dumps(coords);sections.append(o);manifest['helpers'].append(o.Name)
 o=d.addObject('Part::Loft',n);o.Sections=sections;o.Solid=True;o.Ruled=False;manifest['helpers'].append(n);return o
outer=loft('RightOuterContour','wing',[0,.15,.35,.55,.72,.88,1]);inner=loft('RightCavity','wing',[.004,.15,.35,.55,.72,.88,.985],True)
skin=boolean('RightSkin','Cut',outer,inner);left=mirror('LeftSkin',skin);mirror('LeftOuterContour',outer,'helpers')
phase('Adding hollow spars, ribs and separated aileron geometry')
sp=cyl('SparOuter',5,720,(400,0,88),(0,1,0));sp.setExpression('Height','DesignParameters.Span * 0.4')
si=cyl('SparInner',4,722,(400,-1,88),(0,1,0));si.setExpression('Height','DesignParameters.Span * 0.4 + 2 mm')
spar=boolean('RightSpar','Cut',sp,si);mirror('LeftSpar',spar)
for i,eta in enumerate([.05,.2,.36,.52,.68,.78]):
 slab=box('RibSlab'+str(i),400,2,90,(200,eta*900,50));slab.setExpression('Placement.Base.y',f'DesignParameters.Span * {eta/2}')
 rib=boolean('RibBlank'+str(i),'Common',inner,slab,False);rib=boolean('RightRib'+str(i),'Cut',rib,sp);mirror('LeftRib'+str(i),rib)
# Aileron is a genuine separated piece of the original wing envelope. Hinge/servo design remains pending.
cutbox=box('AileronCutVolume',150,430,100,(475,300,40));cutbox.setExpression('Width','DesignParameters.Span * .239');cutbox.setExpression('Placement.Base.y','DesignParameters.Span / 6')
flap=boolean('RightAileron','Common',outer,cutbox);mirror('LeftAileron',flap)
manifest['physical'].remove('RightSkin');manifest['physical'].remove('LeftSkin');manifest['helpers']+=['RightSkin','LeftSkin']
skin=boolean('RightWingSkin','Cut',skin,cutbox);left=mirror('LeftWingSkin',skin)
phase('Building open pod, removable hatch, trays and routing provisions')
podouter=loft('PodOuter','pod',[0,.15,.35,.6,.85,1]);podinner=loft('PodCavity','pod',[.01,.15,.35,.6,.85,.99],True)
podshell=boolean('PodShell','Cut',podouter,podinner,False)
opening=box('HatchOpening',320,92,90,(210,-46,48));pod=boolean('Pod','Cut',podshell,opening)
hatch=boolean('Hatch','Common',podshell,opening)
tray=box('BatteryTray',172,74,3,(225,-37,8),'physical');tray.setExpression('Length','DesignParameters.BatteryLength + 20 mm');tray.setExpression('Width','DesignParameters.BatteryWidth + 18 mm');tray.setExpression('Placement.Base.y','-(DesignParameters.BatteryWidth + 18 mm) / 2')
for i,x in enumerate([245,340]):
 slot=box('BatteryStrapSlot'+str(i),5,78,5,(x,-39,7));slot.setExpression('Width','DesignParameters.BatteryWidth + 22 mm');slot.setExpression('Placement.Base.y','-(DesignParameters.BatteryWidth + 22 mm) / 2');manifest['physical'].remove(tray.Name);manifest['helpers'].append(tray.Name);tray=boolean('BatteryTrayCut'+str(i),'Cut',tray,slot)
# Wiring reservations are nonphysical routing volumes and must not be exported as physical parts.
for sign in [-1,1]:
 c=cyl('WingHarnessReservation'+('Left' if sign<0 else 'Right'),3,720,(365,0,89),(0,sign,0),'references');c.setExpression('Height','DesignParameters.Span * .4')
phase('Creating twin booms and conventional tail/control surfaces')
for side,sign in [('Right',1),('Left',-1)]:
 bo=cyl('BoomOuter'+side,10,950,(65,sign*320,40),(1,0,0));bi=cyl('BoomInner'+side,8,952,(64,sign*320,40),(1,0,0))
 for o in [bo,bi]:o.setExpression('Placement.Base.y',f'DesignParameters.BoomHalfSpacing * {sign}')
 boom=boolean('Boom'+side,'Cut',bo,bi)
 fin=box('Fin'+side,130,5,175,(910,sign*320-2.5,40),'physical');fin.setExpression('Placement.Base.y',f'DesignParameters.BoomHalfSpacing * {sign} - 2.5 mm')
 rud=box('Rudder'+side,38,5,155,(1042,sign*320-2.5,50),'physical');rud.setExpression('Placement.Base.y',f'DesignParameters.BoomHalfSpacing * {sign} - 2.5 mm')
tail=box('Stabilizer',130,645,6,(900,-322.5,215),'physical');tail.setExpression('Width','DesignParameters.BoomHalfSpacing * 2 + 5 mm');tail.setExpression('Placement.Base.y','-DesignParameters.BoomHalfSpacing - 2.5 mm')
elev=box('Elevator',38,600,6,(1032,-300,215),'physical');elev.setExpression('Width','DesignParameters.BoomHalfSpacing * 2 - 40 mm');elev.setExpression('Placement.Base.y','-DesignParameters.BoomHalfSpacing + 20 mm')
phase('Placing sourced candidate hardware and explicitly dimensional reservations')
source_shapes={}
for name in ['mn2806evo','p12x4']:
 file=ROOT/f'cad/components/vendor/tmotor/{name}.step';source_shapes[name]=Part.read(str(file));manifest.setdefault('hardware_sources',{})[name]={'path':str(file),'sha256':hashlib.sha256(file.read_bytes()).hexdigest(),'classification':'Exact manufacturer STEP geometry; proposed component, not frozen compatible kit'}
for side,sign in [('Right',1),('Left',-1)]:
 for end,x in [('Front',120),('Rear',700)]:
  n=side+end
  o=d.addObject('Part::Feature','LiftMotor'+n);o.Shape=source_shapes['mn2806evo'];o.Placement=A.Placement(A.Vector(x,sign*320,150),A.Rotation(A.Vector(1,0,0),A.Vector(0,0,-1)));o.setExpression('Placement.Base.x','DesignParameters.Motor'+end+'X');o.setExpression('Placement.Base.y',f'DesignParameters.BoomHalfSpacing * {sign}');classify(o,'Exact manufacturer BRep; unfrozen candidate, mount unresolved','references')
  prop=d.addObject('Part::Feature','LiftProp'+n);prop.Shape=source_shapes['p12x4'];prop.Placement=A.Placement(A.Vector(x,sign*320,159.2130499798656),A.Rotation(A.Vector(0,0,1),45));prop.setExpression('Placement.Base.x','DesignParameters.Motor'+end+'X');prop.setExpression('Placement.Base.y',f'DesignParameters.BoomHalfSpacing * {sign}');classify(prop,'Exact one-handed manufacturer BRep; displayed handedness not valid four-rotor selection','references')
  sweep=cyl('LiftSweep'+n,153,16,(x,sign*320,151),kind='references');sweep.setExpression('Placement.Base.x','DesignParameters.Motor'+end+'X');sweep.setExpression('Placement.Base.y',f'DesignParameters.BoomHalfSpacing * {sign}')
phase('Building proposed measured-pattern motor plates and split boom saddles')
for side,sign in [('Right',1),('Left',-1)]:
 for end,x in [('Front',120),('Rear',700)]:
  suffix=side+end
  for mode in ['Upper','Lower']:
   mount=d.addObject('Part::FeaturePython','LiftMount'+mode+suffix);vtol_features.LiftMount(mount,mode);mount.Parameters=p;mount.Placement.Base=A.Vector(x,sign*320,0);mount.setExpression('Placement.Base.x','DesignParameters.Motor'+end+'X');mount.setExpression('Placement.Base.y',f'DesignParameters.BoomHalfSpacing * {sign}');classify(mount,'Original provisional split saddle; motor M3 pattern verified from drawing, loads and safe motor screw engagement unresolved')
  for i,offset in enumerate([-20,20]):
   shank=cyl('ClampShank'+suffix+str(i),1.5,25,(x,sign*320+offset,28),(0,0,1))
   head=cyl('ClampHead'+suffix+str(i),2.75,3,(x,sign*320+offset,53))
   bolt=d.addObject('Part::MultiFuse','ClampBolt'+suffix+str(i));bolt.Shapes=[shank,head];classify(bolt,'Simplified proposed M3x25 socket fastener envelope; thread/grade/source not frozen')
   for item in [shank,head]:item.setExpression('Placement.Base.x','DesignParameters.Motor'+end+'X');item.setExpression('Placement.Base.y',f'DesignParameters.BoomHalfSpacing * {sign} + {offset} mm')
   nut=d.addObject('Part::Feature','ClampNut'+suffix+str(i));rad=5.5/math.sqrt(3);pts=[A.Vector(rad*math.cos(j*math.pi/3),rad*math.sin(j*math.pi/3),0) for j in range(6)];nut.Shape=Part.Face(Part.makePolygon(pts+pts[:1])).extrude(A.Vector(0,0,2.4)).cut(Part.makeCylinder(1.5,2.4));nut.Placement.Base=A.Vector(x,sign*320+offset,28.1);nut.setExpression('Placement.Base.x','DesignParameters.Motor'+end+'X');nut.setExpression('Placement.Base.y',f'DesignParameters.BoomHalfSpacing * {sign} + {offset} mm');classify(nut,'Simplified hex nut envelope, AF5.5 height2.4 assumptions; omitted thread, no strength claim')
manifest['motor_mount_evidence']={'source':'package-v2/sources/motor-drawing.pdf','pattern':'2xM3 on19mm and2xM3 on25mm diametral patterns, measured source rotation retained','motor_base_world_z_mm':136.3,'plate_thickness_mm':4,'central_relief_diameter_mm':9,'local_fit_check':'0mm3 common and0mm distance motor/plate, bothclamps/boom;1mm splitgap; initial5mmcentralhole failed7.46mm3 against actualbaseboss and was enlarged to9mm','boom_bore_diameter_mm':20,'split_gap_mm':1,'motor_screw_engagement':'UNRESOLVED; motor fasteners deliberately not fabricated from missing depth data','clamp_fasteners':'Simplified provisional M3x25+nut envelopes; thread mechanics, grade, tightening torque and strength unresolved'}
manifest['unresolved'].append('Motor plates bear on source base but motor screws are not selected: manufacturer does not specify allowable engagement. Clamp thread geometry is simplified; no preload/strength validation.')
bat=box('BatteryReservation',133,45,33.5,(235,-22.5,12),'references')
for field in ['Length','Width','Height']:bat.setExpression(field,'DesignParameters.Battery'+field)
bat.setExpression('Placement.Base.y','-DesignParameters.BatteryWidth / 2')
classify_note=[(bat,'CNHL500654BKZ4S nominal147x51x38mm; reservation includes+5mm each dimension; XT90 connector access unresolved')]
for n,size,pos in [('FlightControllerReservation',(54.3,39,17.5),(425,-19.5,17)),('PowerModuleReservation',(68,50,10),(490,-25,12)),('PayloadReservation',(70,60,35),(140,-30,5)),('CruiseMotorReservation',(45,40,40),(650,-20,10))]:
 o=box(n,*size,pos,'references');classify_note.append((o,'Unfrozen datasheet/package reservation; not imported component CAD'))
for o,note in classify_note:o.addProperty('App::PropertyString','Fidelity');o.Fidelity=note
cyl('CruisePropSweepReservation',153,18,(710,0,50),(1,0,0),'references')
for side,sign in [('Right',1),('Left',-1)]:
 o=box('ServoReservation'+side,24,13,28,(440,sign*520,65),'references');o.setExpression('Length','DesignParameters.ServoLength')
phase('Recomputing native model and recording actual geometry validity')
d.recompute()
invalid=[]
for n in manifest['physical']:
 o=d.getObject(n)
 if o.Shape.isNull() or not o.Shape.isValid() or not o.Shape.Solids:invalid.append(n)
manifest['invalid_physical']=invalid
for o in d.Objects:
 if hasattr(o,'ViewObject') and o.ViewObject:o.ViewObject.Visibility=o.Name in manifest['physical'] or o.Name.startswith(('LiftMotor','LiftProp'))
manifest.update(native=str(out/'assembly.FCStd'),parameter_object='DesignParameters',wing_object='RightOuterContour',proxy_module=str(Path(vtol_features.__file__).resolve()),parameter_scope={'Span':'wing sections/rib spacing/spar length/aileron region','RootChord':'wing sections and cavity; fixed spar/chordwise interfaces may fail after edit','BoomHalfSpacing':'booms/tail/lift hardware placement','BatteryLength':'battery reservation and tray length','ServoLength':'servo reservation only'},contact_pairs=[[f'LiftMount{mode}{side}{end}',f'Boom{side}'] for side in ['Right','Left'] for end in ['Front','Rear'] for mode in ['Upper','Lower']]+[[f'LiftMountUpper{side}{end}',f'LiftMotor{side}{end}'] for side in ['Right','Left'] for end in ['Front','Rear']]+[[f'ClampBolt{side}{end}{i}',f'LiftMountUpper{side}{end}'] for side in ['Right','Left'] for end in ['Front','Rear'] for i in range(2)]+[[f'ClampNut{side}{end}{i}',f'LiftMountLower{side}{end}'] for side in ['Right','Left'] for end in ['Front','Rear'] for i in range(2)],audit_status='pending independent all-pair/prop/structural/electrical checks')
d.recompute();d.saveAs(str(out/'assembly.FCStd'))
module_dir=str(Path(__file__).resolve().parent)
macro='import sys, FreeCAD as App, FreeCADGui as Gui\nsys.path.insert(0,'+repr(module_dir)+')\nimport vtol_features\ndoc=App.openDocument('+repr(str(out/'assembly.FCStd'))+')\ndoc.recompute()\nvisible='+repr(manifest['physical']+[o.Name for o in d.Objects if o.Name.startswith(('LiftMotor','LiftProp'))])+'\nfor obj in doc.Objects:\n if obj.ViewObject: obj.ViewObject.Visibility=obj.Name in visible\nGui.activeDocument().activeView().viewAxonometric()\nGui.activeDocument().activeView().fitAll()\n'
(out/'open.FCMacro').write_text(macro);manifest['open_macro']=str(out/'open.FCMacro')
manifest['collision_objects']=manifest['physical']+[o.Name for o in d.Objects if o.Name.startswith(('LiftMotor','LiftProp'))]
manifest['swept_volume_objects']=[o.Name for o in d.Objects if 'Sweep' in o.Name]
phase('Rendering actual native assembled and internal cutaway views')
visible=[d.getObject(n) for n in manifest['physical'] if n not in invalid]+[o for o in d.Objects if o.Name.startswith(('LiftMotor','LiftProp'))]
for n,objects in [('assembled',visible),('internals',[o for o in visible if o.Name not in ['RightWingSkin','LeftWingSkin','Pod','Hatch']])]:
 try:manifest.setdefault('renders',{})[n]=surface_preview.selected(objects,out/(n+'.png'),width=640,height=480,azimuth=-45,elevation=28)
 except Exception as e:manifest.setdefault('render_failures',{})[n]=str(e)
phase('Rendering native cutaway and exploded diagnostic copies')
try:
 import preview_views
 manifest['diagnostic_views']=preview_views.generate(d,manifest['collision_objects'],out)
except Exception as e:manifest.setdefault('render_failures',{})['diagnostic_views']=str(e)
phase('Exporting proposed physical airframe STEP; equivalence audit pending')
valid=[d.getObject(n) for n in manifest['physical'] if n not in invalid]
Part.export(valid,str(out/'airframe.step'));manifest['step']={'path':str(out/'airframe.step'),'equivalence':'not yet audited','omissions':'COTS candidates and all nonphysical reservations excluded'}
manifest['native_sha256']=hashlib.sha256((out/'assembly.FCStd').read_bytes()).hexdigest();manifest['status']='provisional_layout_built';manifest['elapsed_seconds']=time.time()-start
(out/'artifact-manifest.json').write_text(json.dumps(manifest,indent=2));
if (out/'failure.json').exists():(out/'failure.json').unlink()
print(json.dumps({'status':manifest['status'],'invalid':invalid,'native':manifest['native']}),flush=True)
