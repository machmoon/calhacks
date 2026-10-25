"""Local typed edits on the owning GUI thread. Independent Wright implementation."""
import hashlib,json,math,os,re,time
from pathlib import Path
BOUNDS={'Span':(1600,2000),'RootChord':(280,340),'BoomHalfSpacing':(280,380),'MotorFrontX':(80,140),'MotorRearX':(680,740),'WingletHeight':(40,110),'WingletCantAngle':(5,35),'WingletSweepOffset':(10,60),'TailDihedralAngle':(20,40)}
def validate(values):
 if not isinstance(values,dict) or not values or set(values)-set(BOUNDS):raise ValueError('Only supported numeric design parameters may be edited')
 for key,value in values.items():
  if type(value) not in (int,float) or not math.isfinite(value) or not BOUNDS[key][0]<=value<=BOUNDS[key][1]:raise ValueError('Out of bounds: '+key)
 return values

def write(path,value):
 temporary=path.with_suffix(path.suffix+'.tmp');temporary.write_text(json.dumps(value,indent=2));temporary.replace(path)
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def parameters(doc):return {k:getattr(doc.DesignParameters,k).Value for k in BOUNDS}

def start(doc,gui,folder):
 from PySide6 import QtCore
 folder=Path(folder);queue=folder/'edit-queue';queue.mkdir(exist_ok=True)
 doc.UndoMode=1
 state=dict(pid=os.getpid(),document=doc.Name,revision=digest(folder/'assembly.FCStd'),parameters=parameters(doc),status='ready')
 write(folder/'live-session.json',state)
 def tick():
  for path in sorted(queue.glob('*.request.json'))[:1]:
   identifier=path.name.removesuffix('.request.json')
   if not re.fullmatch('[a-f0-9]{32}',identifier):continue
   target=queue/(identifier+'.result.json')
   if target.exists():path.unlink();continue
   began=time.monotonic();transaction=False;native=folder/('edit-'+identifier+'.FCStd');image=folder/('edit-'+identifier+'.png')
   try:
    if path.stat().st_size>8192:raise ValueError('Oversized edit request')
    request=json.loads(path.read_text());values=validate(request['parameters'])
    if request.get('expected_revision')!=state['revision']:raise ValueError('Stale edit revision; reload live session')
    if parameters(doc)!=state['parameters']:raise ValueError('Parameters changed outside this session; refusing stale overwrite')
    doc.openTransaction('Wright parameter edit');transaction=True
    for key,value in values.items():setattr(doc.DesignParameters,key,value)
    doc.recompute()
    if any('Invalid' in str(o.State) or 'Error' in str(o.State) for o in doc.Objects):raise ValueError('Native recompute failed')
    gui.getDocument(doc.Name).activeView().fitAll();gui.updateGui()
    gui.getDocument(doc.Name).activeView().saveImage(str(image),960,640,'Current')
    doc.saveCopy(str(native));revision=digest(native)
    doc.commitTransaction();transaction=False
    state.update(revision=revision,parameters=parameters(doc),latest_native=str(native));write(folder/'live-session.json',state)
    result=dict(status='edited_unverified',native=str(native),image=str(image),revision=revision,parameters=state['parameters'],worker_elapsed_s=time.monotonic()-began,recompute_count=1,engineering_checks='not_run',step_export='not_updated')
   except Exception as error:
    if transaction:doc.abortTransaction();doc.recompute()
    native.unlink(missing_ok=True);image.unlink(missing_ok=True)
    result=dict(status='failed',error=str(error),worker_elapsed_s=time.monotonic()-began)
   write(target,result);path.unlink()
 timer=QtCore.QTimer();timer.timeout.connect(tick);timer.start(150)
 return timer
