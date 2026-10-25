"""Native proposed VTOL layout. Run with cad/freecad/run.sh; no complete-aircraft claim."""
import argparse,json,hashlib,time,math,sys
from pathlib import Path
import FreeCAD as A,Part
import vtol_features_r9 as vtol_features
import surface_preview,shutil
ROOT=Path(__file__).resolve().parents[3]
a=argparse.ArgumentParser();a.add_argument('--output-dir',required=True);a.add_argument('--package');a.add_argument('--design');a.add_argument('--skip-renders',action='store_true');a=a.parse_args()
out=Path(a.output_dir).resolve();out.mkdir(parents=True,exist_ok=True);start=time.time()
manifest={'schema_version':1,'design_revision':'layout-r9-clean-inverted-v','status':'building','complete_vtol':False,'hardware_ready':False,'physical':[],'references':[],'helpers':[],'unresolved':['Hardware package is not frozen; component geometry does not establish compatibility.','Hardware-specific mounts, screws and installation paths pending verified component interfaces.','Structure and joints are not sized for loads; contact is not strength.','Cavity uses reduced section thickness, not certified constant wall.','Control hinges, linkages and servo travel not verified.','Mass/CG, wiring pinout, thermal design, propulsion and endurance remain pending.','All-pair clearance and connectivity audit separate; no collision-free assertion.'],'package':a.package}
manifest['generator_sources']={str(path):hashlib.sha256(path.read_bytes()).hexdigest() for path in [Path(__file__).resolve(),Path(vtol_features.__file__).resolve(),Path(__file__).resolve().parent/'preview_views.py',Path(__file__).resolve().parent/'export_preview_r8.py']}
if a.package:
 package=Path(a.package)
 if not package.is_dir():raise ValueError('--package must be a package directory')
 manifest['package_sources']={path.name:hashlib.sha256(path.read_bytes()).hexdigest() for path in sorted(package.glob('*.json'))}
manifest['images']={} if a.skip_renders else {n:str(out/(n+'.png')) for n in ['assembled','internals','cutaway','exploded']}
def phase(s):
 manifest['status']=s;manifest['elapsed_seconds']=time.time()-start;(out/'progress.json').write_text(json.dumps(manifest,indent=2));print(s,flush=True)
def fail(typ,error,tb):
 manifest['status']='failed';manifest['error']=str(error);(out/'failure.json').write_text(json.dumps(manifest,indent=2));sys.__excepthook__(typ,error,tb)
sys.excepthook=fail
defaults=dict(WingletHeight=70,WingletCantAngle=20,WingletSweepOffset=30,TailDihedralAngle=30,Span=1800,RootChord=300,PodHalfHeight=46,BoomHalfSpacing=320,BatteryLength=152,BatteryWidth=56,BatteryHeight=43,ServoLength=24,MotorFrontX=120,MotorRearX=700)
ranges=dict(WingletHeight=(40,110),WingletCantAngle=(5,35),WingletSweepOffset=(10,60),TailDihedralAngle=(20,40),Span=(1600,2000),RootChord=(280,340),PodHalfHeight=(42,48),BoomHalfSpacing=(280,380),BatteryLength=(147,165),BatteryWidth=(51,65),BatteryHeight=(38,50),ServoLength=(20,32),MotorFrontX=(80,140),MotorRearX=(680,740))
if a.design:
 supplied=json.loads(Path(a.design).read_text())
 if not isinstance(supplied,dict):raise ValueError('Design must be a JSON object')
 for key,value in supplied.items():
  if key not in defaults or isinstance(value,bool) or not isinstance(value,(int,float)) or not math.isfinite(value) or not ranges[key][0]<=value<=ranges[key][1]:raise ValueError('Unsupported/out-of-range design parameter: '+key)
 defaults.update(supplied)
manifest['design_parameters']=defaults;manifest['design_parameters_mm']={k:v for k,v in defaults.items() if k not in ['TailDihedralAngle','WingletCantAngle']};manifest['input_bounds_not_engineering_validation']=ranges
d=A.newDocument('VTOLDemonstrator');p=d.addObject('App::FeaturePython','DesignParameters')
for n,v in defaults.items():p.addProperty('App::PropertyAngle' if n in ['TailDihedralAngle','WingletCantAngle'] else 'App::PropertyLength',n,'Design');setattr(p,n,v)
p.addProperty('App::PropertyLength','ClampBoltOffset');p.ClampBoltOffset=25;p.setEditorMode('ClampBoltOffset',1)
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
service=[];ribseats=[]
for side,sign in [('Center',0),('Right',1),('Left',-1)]:
 tool=box('WingServiceOpening'+side,36,14,100,(382,sign*p.BoomHalfSpacing.Value-7,60));tool.setExpression('Placement.Base.y',f'DesignParameters.BoomHalfSpacing * {sign} - 7 mm');service.append(tool)
 if sign>0:
  seat=box('WingRibCollarSeat',36,14,18,(382,p.BoomHalfSpacing.Value-7,76));seat.setExpression('Placement.Base.y','DesignParameters.BoomHalfSpacing - 7 mm');ribseats.append(seat)
servicetool=d.addObject('Part::MultiFuse','WingServiceCutters');servicetool.Shapes=service;manifest['helpers'].append(servicetool.Name)
outer=loft('RightOuterContour','wing',[0,.15,.35,.55,.72,.88,1]);inner=loft('RightCavity','wing',[.004,.15,.35,.55,.72,.88,.985],True)
skin=boolean('RightSkin','Cut',outer,inner);left=mirror('LeftSkin',skin);mirror('LeftOuterContour',outer,'helpers')
phase('Adding hollow spars, ribs and separated aileron geometry')
sp=cyl('SparOuter',5,720,(400,0,85),(0,1,0));sp.setExpression('Height','DesignParameters.Span * 0.4')
si=cyl('SparInner',4,722,(400,-1,85),(0,1,0));si.setExpression('Height','DesignParameters.Span * 0.4 + 2 mm')
spar=boolean('RightSpar','Cut',sp,si);mirror('LeftSpar',spar)
ribbay=box('RibControlBayClearance',151,432,100,(474,299,40));ribbay.setExpression('Width','DesignParameters.Span * .239 + 2 mm');ribbay.setExpression('Placement.Base.y','DesignParameters.Span / 6 - 1 mm')
for i,eta in enumerate([.05,.2,.36,.52,.68,.78]):
 slab=box('RibSlab'+str(i),400,2,90,(200,eta*900,50));slab.setExpression('Placement.Base.y',f'DesignParameters.Span * {eta/2}')
 rib=boolean('RibBlank'+str(i),'Common',inner,slab,False);rib=boolean('RibControlTrim'+str(i),'Cut',rib,ribbay,False);rib=boolean('RibSparSeat'+str(i),'Cut',rib,ribseats[0],False);rib=boolean('RightRib'+str(i),'Cut',rib,sp);mirror('LeftRib'+str(i),rib)
# Aileron is a genuine separated piece of the original wing envelope. Hinge/servo design remains pending.
cutbox=box('AileronCutVolume',149,428,100,(476,301,40));cutbox.setExpression('Width','DesignParameters.Span * .239 - 2 mm');cutbox.setExpression('Placement.Base.y','DesignParameters.Span / 6 + 1 mm')
flap=boolean('RightAileron','Common',outer,cutbox);mirror('LeftAileron',flap)
manifest['physical'].remove('RightSkin');manifest['physical'].remove('LeftSkin');manifest['helpers']+=['RightSkin','LeftSkin']
skin=boolean('RightControlSkin','Cut',skin,ribbay,False)
passage=cyl('SparPassage',5.2,721,(400,-.5,85),(0,1,0));passage.setExpression('Height','DesignParameters.Span * .4 + 1 mm')
skin=boolean('RightSparPassageSkin','Cut',skin,passage,False);skin=boolean('RightWingSkin','Cut',skin,servicetool);left=mirror('LeftWingSkin',skin)
phase('Building open pod, removable hatch, trays and routing provisions')
podouter=loft('PodOuter','pod',[0,.15,.35,.6,.85,1]);podinner=loft('PodCavity','pod',[.01,.15,.35,.6,.85,.99],True)
podshell=boolean('PodShell','Cut',podouter,podinner,False)
opening=box('HatchOpening',320,92,90,(210,-46,48));pod=boolean('PodOpenShell','Cut',podshell,opening,False)
hatch=boolean('HatchBlank','Common',podshell,opening,False);hatch=boolean('Hatch','Cut',hatch,service[0])
# Four internal strap windows preserve outer edge bridges; never sever the tray.
tray=box('BatteryTrayBlank',172,74,3,(225,-37,8));tray.setExpression('Length','DesignParameters.BatteryLength + 20 mm');tray.setExpression('Width','DesignParameters.BatteryWidth + 18 mm');tray.setExpression('Placement.Base.y','-(DesignParameters.BatteryWidth + 18 mm) / 2')
traytools=[];podtools=[];bosses=[]
for end in ['Front','Rear']:
 for sign in [-1,1]:
  n=end+('Right' if sign>0 else 'Left');x=245 if end=='Front' else p.BatteryLength.Value+225
  slot=box('StrapWindow'+n,12,4,5,(x-6,sign*(p.BatteryWidth.Value/2+4)-2,7));slot.setExpression('Placement.Base.y',f'(DesignParameters.BatteryWidth / 2 + 4 mm) * {sign} - 2 mm')
  if end=='Rear':slot.setExpression('Placement.Base.x','DesignParameters.BatteryLength + 219 mm')
  traytools.append(slot)
  axis=230 if end=='Front' else p.BatteryLength.Value+239.5;y=sign*(p.BatteryWidth.Value/2+4)
  boss=cyl('TrayBossBlank'+n,5,38,(axis,y,-30));boss.setExpression('Placement.Base.y',f'(DesignParameters.BatteryWidth / 2 + 4 mm) * {sign}')
  if end=='Rear':boss.setExpression('Placement.Base.x','DesignParameters.BatteryLength + 239.5 mm')
  bosses.append(boolean('TrayBoss'+n,'Common',podouter,boss,False))
  hole=cyl('TrayBore'+n,1.6,15,(axis,y,0));hole.setExpression('Placement.Base.y',f'(DesignParameters.BatteryWidth / 2 + 4 mm) * {sign}')
  if end=='Rear':hole.setExpression('Placement.Base.x','DesignParameters.BatteryLength + 239.5 mm')
  traytools.append(hole);podtools.append(hole)
  # Side-loaded captive nut below an intact3mm boss ceiling.
  pocket=box('TrayNutSideEntry'+n,6.6,10,2.4,(axis-3.3,y-(10 if sign>0 else 0),2.6));pocket.setExpression('Placement.Base.y',f'(DesignParameters.BatteryWidth / 2 + 4 mm) * {sign} - {10 if sign>0 else 0} mm')
  if end=='Rear':pocket.setExpression('Placement.Base.x','DesignParameters.BatteryLength + 236.2 mm')
  podtools.append(pocket)
  trap=d.addObject('Part::Feature','TrayNutPocket'+n);rad=5.6/math.sqrt(3);pts=[A.Vector(rad*math.cos(j*math.pi/3),rad*math.sin(j*math.pi/3),0) for j in range(6)];trap.Shape=Part.Face(Part.makePolygon(pts+pts[:1])).extrude(A.Vector(0,0,2.4));trap.Placement.Base=A.Vector(axis,y,2.6);trap.setExpression('Placement.Base.y',f'(DesignParameters.BatteryWidth / 2 + 4 mm) * {sign}')
  if end=='Rear':trap.setExpression('Placement.Base.x','DesignParameters.BatteryLength + 239.5 mm')
  podtools.append(trap);manifest['helpers'].append(trap.Name)
  shank=cyl('TrayBoltShank'+n,1.5,10,(axis,y,1));head=cyl('TrayBoltHead'+n,2.75,3,(axis,y,11))
  for obj in [shank,head]:
   obj.setExpression('Placement.Base.y',f'(DesignParameters.BatteryWidth / 2 + 4 mm) * {sign}')
   if end=='Rear':obj.setExpression('Placement.Base.x','DesignParameters.BatteryLength + 239.5 mm')
  bolt=d.addObject('Part::MultiFuse','TrayBolt'+n);bolt.Shapes=[shank,head];classify(bolt,'Simplified provisional M3x10 tray fastener; omitted thread mechanics')
  nut=d.addObject('Part::Feature','TrayNut'+n);rad=5.5/math.sqrt(3);pts=[A.Vector(rad*math.cos(j*math.pi/3),rad*math.sin(j*math.pi/3),0) for j in range(6)];nut.Shape=Part.Face(Part.makePolygon(pts+pts[:1])).extrude(A.Vector(0,0,2.4)).cut(Part.makeCylinder(1.5,2.4));nut.Placement.Base=A.Vector(axis,y,2.6);nut.setExpression('Placement.Base.y',f'(DesignParameters.BatteryWidth / 2 + 4 mm) * {sign}')
  if end=='Rear':nut.setExpression('Placement.Base.x','DesignParameters.BatteryLength + 239.5 mm')
  classify(nut,'Simplified side-loaded captive nut; source/grade and thread strength unresolved')
traytool=d.addObject('Part::MultiFuse','TrayCutters');traytool.Shapes=traytools;manifest['helpers'].append(traytool.Name);tray=boolean('BatteryTray','Cut',tray,traytool)
framebox=box('PodFrameVolume',8,160,80,(396,-80,-30));frame=boolean('PodInternalFrameRaw','Common',podouter,framebox,False)
trayclear=box('TrayFrameClearance',172.4,74.4,3.4,(224.8,-37.2,7.8));trayclear.setExpression('Length','DesignParameters.BatteryLength + 20.4 mm');trayclear.setExpression('Width','DesignParameters.BatteryWidth + 18.4 mm');trayclear.setExpression('Placement.Base.y','-(DesignParameters.BatteryWidth + 18.4 mm) / 2');frame=boolean('PodInternalFrame','Cut',frame,trayclear,False)
for sign in [-1,1]:podtools.append(cyl('FrameCablePassage'+str(sign).replace('-','Minus'),4,12,(394,sign*25,20),(1,0,0)))
rootbody=d.addObject('Part::FeaturePython','PodSparBridge');vtol_features.WingBridge(rootbody,'PodBody');rootbody.Parameters=p;rootbody.Placement.Base=A.Vector(400,0,0);manifest['helpers'].append(rootbody.Name)
podfuse=d.addObject('Part::MultiFuse','PodStructuralFuse');podfuse.Shapes=[pod,frame,rootbody]+bosses;manifest['helpers'].append(podfuse.Name)
podtool=d.addObject('Part::MultiFuse','PodInternalCutters');podtool.Shapes=podtools;manifest['helpers'].append(podtool.Name);pod=boolean('Pod','Cut',podfuse,podtool)
joiner=cyl('CenterSparJoiner',4,80,(400,-40,85),(0,1,0),'physical');joiner.addProperty('App::PropertyString','Fidelity');joiner.Fidelity='Proposed8mm nominal joiner rod; clamp/friction and material not structurally qualified'
# Shared split caps and their explicit fastener bearing geometry.
for side,sign in [('Center',0),('Right',1),('Left',-1)]:
 y=sign*p.BoomHalfSpacing.Value
 cap=d.addObject('Part::FeaturePython','WingSparCap'+side);vtol_features.WingBridge(cap,'Cap');cap.Parameters=p;cap.Placement.Base=A.Vector(400,y,0);cap.setExpression('Placement.Base.y',f'DesignParameters.BoomHalfSpacing * {sign}');classify(cap,'Original provisional split spar cap')
 if sign:
  body=d.addObject('Part::FeaturePython','WingBoomBody'+side);vtol_features.WingBridge(body,'BoomBody');body.Parameters=p;body.Placement.Base=A.Vector(400,y,0);body.setExpression('Placement.Base.y',f'DesignParameters.BoomHalfSpacing * {sign}');classify(body,'Original provisional retained boom-to-spar bridge')
  low=d.addObject('Part::FeaturePython','WingBoomLower'+side);vtol_features.LiftMount(low,'Lower');low.Parameters=p;low.Placement.Base=A.Vector(400,y,0);low.setExpression('Placement.Base.y',f'DesignParameters.BoomHalfSpacing * {sign}');classify(low,'Original provisional lower boom clamp')
 for i,dx in enumerate([-12,12]):
  shank=cyl('SparBoltShank'+side+str(i),1.5,16,(400+dx,y,75));head=cyl('SparBoltHead'+side+str(i),2.75,3,(400+dx,y,91))
  for item in [shank,head]:item.setExpression('Placement.Base.y',f'DesignParameters.BoomHalfSpacing * {sign}')
  bolt=d.addObject('Part::MultiFuse','SparBolt'+side+str(i));bolt.Shapes=[shank,head];classify(bolt,'Simplified provisional M3x16 cap fastener')
  nut=d.addObject('Part::Feature','SparNut'+side+str(i));rad=5.5/math.sqrt(3);pts=[A.Vector(rad*math.cos(j*math.pi/3),rad*math.sin(j*math.pi/3),0) for j in range(6)];nut.Shape=Part.Face(Part.makePolygon(pts+pts[:1])).extrude(A.Vector(0,0,2.4)).cut(Part.makeCylinder(1.5,2.4));nut.Placement.Base=A.Vector(400+dx,y,77.1);nut.setExpression('Placement.Base.y',f'DesignParameters.BoomHalfSpacing * {sign}');classify(nut,'Simplified provisional captive spar nut')
 if sign:
  for i,dy in enumerate([-25,25]):
   shank=cyl('BridgeBoltShank'+side+str(i),1.5,25,(400,y+dy,30.5));head=cyl('BridgeBoltHead'+side+str(i),2.75,3,(400,y+dy,27.5))
   for item in [shank,head]:item.setExpression('Placement.Base.y',f'DesignParameters.BoomHalfSpacing * {sign} + {dy} mm')
   bolt=d.addObject('Part::MultiFuse','BridgeBolt'+side+str(i));bolt.Shapes=[shank,head];classify(bolt,'Simplified underside-access boom clamp bolt')
   nut=d.addObject('Part::Feature','BridgeNut'+side+str(i));rad=5.5/math.sqrt(3);pts=[A.Vector(rad*math.cos(j*math.pi/3),rad*math.sin(j*math.pi/3),0) for j in range(6)];nut.Shape=Part.Face(Part.makePolygon(pts+pts[:1])).extrude(A.Vector(0,0,2.4)).cut(Part.makeCylinder(1.5,2.4));nut.Placement.Base=A.Vector(400,y+dy,50.6);nut.setExpression('Placement.Base.y',f'DesignParameters.BoomHalfSpacing * {sign} + {dy} mm');classify(nut,'Simplified upper captured boom clamp nut')
# Wiring reservations are nonphysical routing volumes and must not be exported as physical parts.
for sign in [-1,1]:
 c=cyl('WingHarnessReservation'+('Left' if sign<0 else 'Right'),3,720,(365,0,89),(0,sign,0),'references');c.setExpression('Height','DesignParameters.Span * .4')
phase('Creating twin booms and two inclined inverted-V airfoil surfaces')
for side,sign in [('Right',1),('Left',-1)]:
 bo=cyl('BoomOuter'+side,10,950,(65,sign*320,40),(1,0,0));bi=cyl('BoomInner'+side,8,952,(64,sign*320,40),(1,0,0))
 for o in [bo,bi]:o.setExpression('Placement.Base.y',f'DesignParameters.BoomHalfSpacing * {sign}')
 boom=boolean('Boom'+side,'Cut',bo,bi)
 if sign==1:
  fin=d.addObject('Part::FeaturePython','TailSurfaceRight');vtol_features.InvertedVTail(fin);fin.Parameters=p;fin.Placement.Base=A.Vector(950,320,0);fin.setExpression('Placement.Base.y','DesignParameters.BoomHalfSpacing');classify(fin,'Original NACA0012-section inverted-V half with integrated split boom clamp')
  mirror('TailSurfaceLeft',fin)
  control=d.addObject('Part::FeaturePython','RuddervatorRight');vtol_features.InvertedVTail(control,True);control.Parameters=p;control.Placement.Base=A.Vector(950,320,0);control.setExpression('Placement.Base.y','DesignParameters.BoomHalfSpacing');classify(control,'Separate trailing ruddervator; hinges/linkage/control mixing unresolved');mirror('RuddervatorLeft',control)
 lower=d.addObject('Part::FeaturePython','TailClampLower'+side);vtol_features.TailSaddle(lower,'Lower');lower.Parameters=p;lower.Placement.Base=A.Vector(950,sign*320,0);lower.setExpression('Placement.Base.y',f'DesignParameters.BoomHalfSpacing * {sign}');classify(lower,'Original provisional fin lower clamp')
 for i,offset in enumerate([-25,25]):
  shank=cyl('TailClampShank'+side+str(i),1.5,25,(950,sign*320+offset,28));head=cyl('TailClampHead'+side+str(i),2.75,3,(950,sign*320+offset,53))
  for item in [shank,head]:item.setExpression('Placement.Base.y',f'DesignParameters.BoomHalfSpacing * {sign} + DesignParameters.ClampBoltOffset * {1 if offset>0 else -1}')
  bolt=d.addObject('Part::MultiFuse','TailClampBolt'+side+str(i));bolt.Shapes=[shank,head];classify(bolt,'Simplified provisional clamp fastener envelope; omitted threads')
  nut=d.addObject('Part::Feature','TailClampNut'+side+str(i));rad=5.5/math.sqrt(3);pts=[A.Vector(rad*math.cos(j*math.pi/3),rad*math.sin(j*math.pi/3),0) for j in range(6)];nut.Shape=Part.Face(Part.makePolygon(pts+pts[:1])).extrude(A.Vector(0,0,2.4)).cut(Part.makeCylinder(1.5,2.4));nut.Placement.Base=A.Vector(950,sign*320+offset,28.1);nut.setExpression('Placement.Base.y',f'DesignParameters.BoomHalfSpacing * {sign} + DesignParameters.ClampBoltOffset * {1 if offset>0 else -1}');classify(nut,'Simplified provisional nut envelope; omitted threads')
winglet=d.addObject('Part::FeaturePython','WingletRight');vtol_features.Winglet(winglet);winglet.Parameters=p;winglet.Coordinates=json.dumps(coords);classify(winglet,'Original solid smooth-transition winglet; attachment retention unresolved')
mirror('WingletLeft',winglet)

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
  for i,offset in enumerate([-25,25]):
   shank=cyl('ClampShank'+suffix+str(i),1.5,25,(x,sign*320+offset,28),(0,0,1))
   head=cyl('ClampHead'+suffix+str(i),2.75,3,(x,sign*320+offset,53))
   bolt=d.addObject('Part::MultiFuse','ClampBolt'+suffix+str(i));bolt.Shapes=[shank,head];classify(bolt,'Simplified proposed M3x25 socket fastener envelope; thread/grade/source not frozen')
   for item in [shank,head]:item.setExpression('Placement.Base.x','DesignParameters.Motor'+end+'X');item.setExpression('Placement.Base.y',f'DesignParameters.BoomHalfSpacing * {sign} + DesignParameters.ClampBoltOffset * {1 if offset>0 else -1}')
   nut=d.addObject('Part::Feature','ClampNut'+suffix+str(i));rad=5.5/math.sqrt(3);pts=[A.Vector(rad*math.cos(j*math.pi/3),rad*math.sin(j*math.pi/3),0) for j in range(6)];nut.Shape=Part.Face(Part.makePolygon(pts+pts[:1])).extrude(A.Vector(0,0,2.4)).cut(Part.makeCylinder(1.5,2.4));nut.Placement.Base=A.Vector(x,sign*320+offset,28.1);nut.setExpression('Placement.Base.x','DesignParameters.Motor'+end+'X');nut.setExpression('Placement.Base.y',f'DesignParameters.BoomHalfSpacing * {sign} + DesignParameters.ClampBoltOffset * {1 if offset>0 else -1}');classify(nut,'Simplified hex nut envelope, AF5.5 height2.4 assumptions; omitted thread, no strength claim')
manifest['motor_mount_evidence']={'source':'package-v2/sources/motor-drawing.pdf','pattern':'2xM3 on19mm and2xM3 on25mm diametral patterns, measured source rotation retained','motor_base_world_z_mm':136.3,'plate_thickness_mm':4,'central_relief_diameter_mm':9,'local_fit_check':'0mm3 common and0mm distance motor/plate, bothclamps/boom;1mm splitgap; initial5mmcentralhole failed7.46mm3 against actualbaseboss and was enlarged to9mm','boom_bore_diameter_mm':20,'split_gap_mm':1,'motor_screw_engagement':'UNRESOLVED; motor fasteners deliberately not fabricated from missing depth data','clamp_fasteners':'Simplified provisional M3x25+nut envelopes; thread mechanics, grade, tightening torque and strength unresolved'}
manifest['unresolved'].append('Wing service apertures are open; covers, control hinges/linkages and battery straps remain unresolved. Clamped load paths have no strength/preload qualification.')
manifest['unresolved'].append('Motor plates bear on source base but motor screws are not selected: manufacturer does not specify allowable engagement. Clamp thread geometry is simplified; no preload/strength validation.')
bat=box('BatteryReservation',133,45,33.5,(235,-22.5,12),'references')
for field in ['Length','Width','Height']:bat.setExpression(field,'DesignParameters.Battery'+field)
bat.setExpression('Placement.Base.y','-DesignParameters.BatteryWidth / 2')
classify_note=[(bat,'CNHL500654BKZ4S nominal147x51x38mm; reservation includes+5mm each dimension; XT90 connector access unresolved')]
for n,size,pos in [('FlightControllerReservation',(54.3,39,17.5),(425,-19.5,17)),('PowerModuleReservation',(68,50,10),(490,-25,12)),('PayloadReservation',(70,60,35),(140,-30,5))]:
 o=box(n,*size,pos,'references');classify_note.append((o,'Unfrozen datasheet/package reservation; not imported component CAD'))
for o,note in classify_note:o.addProperty('App::PropertyString','Fidelity');o.Fidelity=note
cyl('CruisePropSweepReservation',139.7,48,(675,0,24),(1,0,0),'references')
firewall=cyl('CruiseFirewallBlank',22,6,(640,0,24),(1,0,0))
firewalltool=cyl('CruiseFirewallShaftTool',2.75,8,(639,0,24),(1,0,0))
firewall=boolean('CruiseFirewall','Cut',firewall,firewalltool)
for i,(y,z) in enumerate([(9.5,0),(-9.5,0),(0,12.5),(0,-12.5)]):
 tool=cyl('CruiseBaseHole'+str(i),1.6,8,(639,y,24+z),(1,0,0));new=boolean('CruiseFirewallCut'+str(i),'Cut',firewall,tool);manifest['physical'].remove(firewall.Name);manifest['helpers'].append(firewall.Name);firewall=new
body=cyl('CruiseMotorBody',17.6,36,(646,0,24),(1,0,0))
shaft=cyl('CruiseMotorShaft',2.5,54.5,(643,0,24),(1,0,0))
motor=d.addObject('Part::Fuse','CruiseMotor');motor.Base=body;motor.Tool=shaft;classify(motor,'AT2814 drawing-derived body/shaft envelope; not exact CAD','references')
prop=d.addObject('Part::FeaturePython','CruiseProp');vtol_features.CruiseProp(prop);prop.StationData=json.dumps(json.loads((ROOT/'cad/projects/vtol-demonstrator/package-v2/cruise-prop-stations.json').read_text())['stations']);classify(prop,'APC published stations with approximate elliptical sections/hub; not production CAD','references')
manifest['tail_configuration']='inverted_v'
manifest['tail_objects']=['TailSurfaceRight','TailSurfaceLeft','RuddervatorRight','RuddervatorLeft']
manifest['tail_surface_roles']={'TailSurfaceRight':'fixed right inclined airfoil and boom clamp','TailSurfaceLeft':'fixed left inclined airfoil and boom clamp','RuddervatorRight':'right movable trailing surface; hinges pending','RuddervatorLeft':'left movable trailing surface; hinges pending'}
manifest['unresolved'] += ['Inverted-V center joint retention, ruddervator hinges/linkage/mixing and combined travel remain unresolved. NACA0012 vertical section definition is an authored approximation; tail sizing/stability not verified.']
manifest['parameter_units']={key:('deg' if key in ['TailDihedralAngle','WingletCantAngle'] else 'mm') for key in defaults}
manifest['unresolved'] += ['Winglet root attachment retention and structural sizing unresolved.','Cruise pusher adapter, screw engagement, firewall retention and landing ground clearance unresolved; source station sections approximate.']

for side,sign in [('Right',1),('Left',-1)]:
 o=box('ServoReservation'+side,24,13,28,(440,sign*520,65),'references');o.setExpression('Length','DesignParameters.ServoLength')
phase('Recomputing native model and recording actual geometry validity')
d.recompute()
invalid=[]
for n in manifest['physical']:
 o=d.getObject(n)
 if o.Shape.isNull() or not o.Shape.isValid() or len(o.Shape.Solids)!=1:invalid.append(n)
manifest['invalid_physical']=invalid
manifest['physical_solid_counts']={n:len(d.getObject(n).Shape.Solids) for n in manifest['physical']}
if invalid:
 d.saveAs(str(out/'invalid-candidate.FCStd'));raise ValueError('Physical parts must be valid single connected solids: '+str(invalid))
for o in d.Objects:
 if hasattr(o,'ViewObject') and o.ViewObject:o.ViewObject.Visibility=o.Name in manifest['physical'] or (o.Name.startswith(('LiftMotor','LiftProp')) or o.Name in ['CruiseMotor','CruiseProp'])
manifest.update(native=str(out/'assembly.FCStd'),parameter_object='DesignParameters',wing_object='RightOuterContour',proxy_module=str(Path(vtol_features.__file__).resolve()),parameter_scope={'Span':'wing sections/rib spacing/spar length/aileron region','RootChord':'wing sections and cavity; fixed spar/chordwise interfaces may fail after edit','BoomHalfSpacing':'booms/tail/lift hardware placement','BatteryLength':'battery reservation and tray length','ServoLength':'servo reservation only'},contact_pairs=[[f'LiftMount{mode}{side}{end}',f'Boom{side}'] for side in ['Right','Left'] for end in ['Front','Rear'] for mode in ['Upper','Lower']]+[[f'LiftMountUpper{side}{end}',f'LiftMotor{side}{end}'] for side in ['Right','Left'] for end in ['Front','Rear']]+[[f'ClampBolt{side}{end}{i}',f'LiftMountUpper{side}{end}'] for side in ['Right','Left'] for end in ['Front','Rear'] for i in range(2)]+[[f'ClampNut{side}{end}{i}',f'LiftMountLower{side}{end}'] for side in ['Right','Left'] for end in ['Front','Rear'] for i in range(2)],audit_status='pending independent all-pair/prop/structural/electrical checks')
manifest['contact_pairs'].extend([['WingBoomBodyRight', 'BoomRight'], ['WingBoomLowerRight', 'BoomRight'], ['WingBoomBodyRight', 'RightSpar'], ['WingSparCapRight', 'RightSpar'], ['Pod', 'RightSpar'], ['CenterSparJoiner', 'RightSpar'], ['WingSparCapCenter', 'RightSpar'], ['BridgeBoltRight0', 'WingBoomLowerRight'], ['BridgeNutRight0', 'WingBoomBodyRight'], ['BridgeBoltRight1', 'WingBoomLowerRight'], ['BridgeNutRight1', 'WingBoomBodyRight'], ['WingBoomBodyLeft', 'BoomLeft'], ['WingBoomLowerLeft', 'BoomLeft'], ['WingBoomBodyLeft', 'LeftSpar'], ['WingSparCapLeft', 'LeftSpar'], ['Pod', 'LeftSpar'], ['CenterSparJoiner', 'LeftSpar'], ['WingSparCapCenter', 'LeftSpar'], ['BridgeBoltLeft0', 'WingBoomLowerLeft'], ['BridgeNutLeft0', 'WingBoomBodyLeft'], ['BridgeBoltLeft1', 'WingBoomLowerLeft'], ['BridgeNutLeft1', 'WingBoomBodyLeft'], ['SparBoltCenter0', 'WingSparCapCenter'], ['SparNutCenter0', 'Pod'], ['SparBoltCenter1', 'WingSparCapCenter'], ['SparNutCenter1', 'Pod'], ['SparBoltRight0', 'WingSparCapRight'], ['SparNutRight0', 'WingBoomBodyRight'], ['SparBoltRight1', 'WingSparCapRight'], ['SparNutRight1', 'WingBoomBodyRight'], ['SparBoltLeft0', 'WingSparCapLeft'], ['SparNutLeft0', 'WingBoomBodyLeft'], ['SparBoltLeft1', 'WingSparCapLeft'], ['SparNutLeft1', 'WingBoomBodyLeft'], ['Pod', 'BatteryTray'], ['TrayBoltFrontRight', 'BatteryTray'], ['TrayNutFrontRight', 'Pod'], ['TrayBoltFrontLeft', 'BatteryTray'], ['TrayNutFrontLeft', 'Pod'], ['TrayBoltRearRight', 'BatteryTray'], ['TrayNutRearRight', 'Pod'], ['TrayBoltRearLeft', 'BatteryTray'], ['TrayNutRearLeft', 'Pod']])
d.recompute();d.saveAs(str(out/'assembly.FCStd'))
module_file=Path(vtol_features.__file__).resolve();shutil.copyfile(module_file,out/module_file.name)
manifest['generator_sources'][str((out/module_file.name).resolve())]=hashlib.sha256((out/module_file.name).read_bytes()).hexdigest()
manifest['freecad_version']=A.Version()
module_dir=str(out)
manifest['proxy_module']=str(out/module_file.name);manifest['portable_support_files']=[module_file.name]
macro='import sys, os, FreeCAD as App, FreeCADGui as Gui\nbundle=os.path.dirname(os.path.abspath(__file__))\nsys.path.insert(0,bundle)\nimport vtol_features_r9\ndoc=App.openDocument(os.path.join(bundle,"assembly.FCStd"))\ndoc.recompute()\nvisible='+repr(manifest['physical']+[o.Name for o in d.Objects if (o.Name.startswith(('LiftMotor','LiftProp')) or o.Name in ['CruiseMotor','CruiseProp'])])+'\nfor obj in doc.Objects:\n if obj.ViewObject: obj.ViewObject.Visibility=obj.Name in visible\nGui.activeDocument().activeView().viewAxonometric()\nGui.activeDocument().activeView().fitAll()\n'
macro+='\nfrom PySide6 import QtCore, QtWidgets\ndef activate_model_view():\n App.setActiveDocument(doc.Name)\n mdi=Gui.getMainWindow().findChild(QtWidgets.QMdiArea)\n if mdi:\n  for sub in mdi.subWindowList():\n   if doc.Label in sub.windowTitle() or doc.Name in sub.windowTitle():\n    mdi.setActiveSubWindow(sub)\n Gui.activeDocument().activeView().fitAll()\nQtCore.QTimer.singleShot(2000,activate_model_view)\nQtCore.QTimer.singleShot(5000,activate_model_view)\n'
macro+="\nimport json\ndef capture_active_native_view():\n result={'status':'failed','document':doc.Name,'objects':len(doc.Objects)}\n try:\n  activate_model_view()\n  mdi=Gui.getMainWindow().findChild(QtWidgets.QMdiArea)\n  sub=mdi.activeSubWindow() if mdi else None\n  if sub is None or not (doc.Label in sub.windowTitle() or doc.Name in sub.windowTitle()):\n   raise RuntimeError('Native document subwindow is not active')\n  Gui.activeDocument().activeView().viewAxonometric()\n  Gui.activeDocument().activeView().fitAll()\n  Gui.updateGui()\n  target=os.path.join(bundle,'native-viewport.png')\n  Gui.activeDocument().activeView().saveImage(target,1280,720,'Current')\n  result.update(status='success',image=target,active_window_title=sub.windowTitle())\n except Exception as error:\n  result['error']=str(error)\n with open(os.path.join(bundle,'gui-open.json'),'w') as f:json.dump(result,f,indent=2)\nQtCore.QTimer.singleShot(5000,capture_active_native_view)\n"
(out/'open.FCMacro').write_text(macro);manifest['open_macro']=str(out/'open.FCMacro')
manifest['collision_objects']=manifest['physical']+[o.Name for o in d.Objects if (o.Name.startswith(('LiftMotor','LiftProp')) or o.Name in ['CruiseMotor','CruiseProp'])]
manifest['swept_volume_objects']=[o.Name for o in d.Objects if 'Sweep' in o.Name]
manifest['renders_skipped']=bool(a.skip_renders)
if not a.skip_renders:
 phase('Rendering actual native assembled and internal cutaway views')
 visible=[d.getObject(n) for n in manifest['physical'] if n not in invalid]+[o for o in d.Objects if (o.Name.startswith(('LiftMotor','LiftProp')) or o.Name in ['CruiseMotor','CruiseProp'])]
 for n,objects in [('assembled',visible),('internals',[o for o in visible if o.Name not in ['RightWingSkin','LeftWingSkin','Pod','Hatch']])]:
  try:manifest.setdefault('renders',{})[n]=surface_preview.selected(objects,out/(n+'.png'),width=640,height=480,azimuth=-45,elevation=28)
  except Exception as e:manifest.setdefault('render_failures',{})[n]=str(e)
 phase('Rendering native cutaway and exploded diagnostic copies')
 try:
  import preview_views
  manifest['diagnostic_views']=preview_views.generate(d,manifest['collision_objects'],out)
 except Exception as e:manifest.setdefault('render_failures',{})['diagnostic_views']=str(e)
phase('Exporting bounded native display mesh')
try:
 import export_preview_r8 as export_preview
 manifest['preview_mesh']=export_preview.generate(d,manifest['collision_objects'],out)
except Exception as e:manifest['preview_mesh_error']=str(e)
phase('Exporting proposed physical airframe STEP; equivalence audit pending')
valid=[d.getObject(n) for n in manifest['physical'] if n not in invalid]
Part.export(valid,str(out/'airframe.step'));manifest['step']={'path':str(out/'airframe.step'),'equivalence':'not yet audited','omissions':'COTS candidates and all nonphysical reservations excluded'}
manifest['native_sha256']=hashlib.sha256((out/'assembly.FCStd').read_bytes()).hexdigest();manifest['status']='provisional_layout_built';manifest['elapsed_seconds']=time.time()-start
(out/'artifact-manifest.json').write_text(json.dumps(manifest,indent=2));
if (out/'failure.json').exists():(out/'failure.json').unlink()
print(json.dumps({'status':manifest['status'],'invalid':invalid,'native':manifest['native']}),flush=True)
