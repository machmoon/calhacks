"""Execute a Wright-generated edit inside real FreeCAD, recording real checkpoints.

This host does not construct the camera mount. The submitted model module does.
Launch by exec'ing this file in FreeCAD's Python console. Configuration is local
out/so101-camera-mount/live-config.json; no user source file is overwritten.
"""
import hashlib, importlib.util, json, traceback
from pathlib import Path
from datetime import datetime, timezone
import FreeCAD as App
import FreeCADGui as Gui
from PySide import QtCore, QtWidgets
ROOT=Path('/Users/patliu/Desktop/Coding/wright')
config=json.loads((ROOT/'out/so101-camera-mount/live-config.json').read_text())
out=Path(config['output_dir']);out.mkdir(parents=True,exist_ok=True)
frames=out/'live-checkpoints';frames.mkdir(exist_ok=True)
source=Path(config['input_path']);original=hashlib.sha256(source.read_bytes()).hexdigest()
module_path=Path(config['candidate_path']);module_hash=hashlib.sha256(module_path.read_bytes()).hexdigest()
interface=json.loads((ROOT/'out/so101-camera-mount/interface/measurements.json').read_text())
events=[]
video_frames=out/'viewport-frames';video_frames.mkdir(exist_ok=True)
frame_number=0

def dwell(ms):
    global frame_number
    for _ in range(max(1,ms//200)):
        Gui.activeDocument().activeView().saveImage(str(video_frames/f'{frame_number:05d}.png'),1280,800,'Current')
        frame_number+=1
        loop=QtCore.QEventLoop();QtCore.QTimer.singleShot(200,loop.quit);loop.exec_()

def checkpoint(label):
    doc=App.ActiveDocument
    if not doc:raise RuntimeError('No active document at modeling checkpoint')
    doc.recompute()
    visibility={o.Name:o.ViewObject.Visibility for o in doc.Objects}
    for obj in doc.Objects:
        if obj.TypeId in ('App::Part','Mesh::Feature'): obj.ViewObject.Visibility=True
        elif obj.TypeId in ('App::Origin','App::Line','App::Plane','App::Point'): obj.ViewObject.Visibility=False
    Gui.Selection.clearSelection()
    view=Gui.activeDocument().activeView()
    if not events:
        view.viewAxonometric();view.fitAll()
    for obj in doc.Objects:
        if obj.TypeId=='Mesh::Feature':
            obj.ViewObject.ShapeColor=(.68,.72,.74)
        elif obj.Name=='CameraMount':
            obj.ViewObject.ShapeColor=(.95,.53,.17)
        elif obj.Name=='CameraReferenceEnvelope':
            obj.ViewObject.ShapeColor=(.18,.55,.30);obj.ViewObject.Transparency=35
        elif obj.Name=='CameraRibbonClearance':
            obj.ViewObject.ShapeColor=(.30,.65,.90);obj.ViewObject.Transparency=75
    Gui.getMainWindow().statusBar().showMessage('Wright · '+str(label),0)
    print('WRIGHT CAD STEP:',label,flush=True)
    Gui.updateGui();QtWidgets.QApplication.processEvents()
    view.viewAxonometric();view.fitAll()
    view.redraw();dwell(1800)
    sequence=len(events)+1
    image=frames/f'{sequence:02d}-freecad.png'
    view.saveImage(str(image),1600,1000,'Current')
    Gui.getMainWindow().grab().save(str(frames/f'{sequence:02d}-window.png'))
    shapes=[]
    for obj in doc.Objects:
        if obj.Name.startswith('Camera') and hasattr(obj,'Shape') and not obj.Shape.isNull():
            shapes.append({'name':obj.Name,'type':obj.TypeId,'volume_mm3':obj.Shape.Volume,'solids':len(obj.Shape.Solids)})
    events.append({'step':sequence,'label':str(label),'utc':datetime.now(timezone.utc).isoformat(),'image':str(image),'document':doc.Name,'object_count':len(doc.Objects),'new_shapes':shapes})
    (out/'live-events.json').write_text(json.dumps(events,indent=2))
    for _ in range(2):
        for obj in doc.Objects:
            if obj.Name in visibility: obj.ViewObject.Visibility=visibility[obj.Name]

try:
    Gui.getMainWindow().showMaximized();Gui.getMainWindow().raise_();Gui.getMainWindow().activateWindow()
    spec=importlib.util.spec_from_file_location('wright_camera_candidate',module_path)
    candidate=importlib.util.module_from_spec(spec);spec.loader.exec_module(candidate)
    result=candidate.build(str(source),str(out),checkpoint)
    checkpoint('Completed camera mount — editable native CAD')
    report={'status':'completed','result':result,'candidate_sha256':module_hash,'input_sha256':original,'input_unchanged':hashlib.sha256(source.read_bytes()).hexdigest()==original,'checkpoints':len(events),'capture_kind':'Native FreeCAD viewport frames captured during candidate execution; no replay geometry'}
except Exception:
    report={'status':'failed','traceback':traceback.format_exc(),'candidate_sha256':module_hash,'input_unchanged':hashlib.sha256(source.read_bytes()).hexdigest()==original,'checkpoints':len(events)}
    print(report['traceback'],flush=True)
(out/'live-result.json').write_text(json.dumps(report,indent=2))
print('WRIGHT LIVE RESULT',report['status'],flush=True)
