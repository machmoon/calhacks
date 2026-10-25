"""Native transaction and stale-revision controls; no GUI window needed."""
import json,tempfile,time,uuid,hashlib
from pathlib import Path
import FreeCAD as App
from PySide6 import QtCore
import live_edits
app=QtCore.QCoreApplication.instance() or QtCore.QCoreApplication([])
doc=App.newDocument('LiveEditControls');p=doc.addObject('App::FeaturePython','DesignParameters')
for key,(low,high) in live_edits.BOUNDS.items():
 p.addProperty('App::PropertyLength',key);setattr(p,key,(low+high)/2)
box=doc.addObject('Part::Box','Dependent');box.setExpression('Length','DesignParameters.Span');doc.recompute()
with tempfile.TemporaryDirectory() as folder:
 folder=Path(folder);doc.saveAs(str(folder/'assembly.FCStd'))
 timer=live_edits.start(doc,None,folder)
 state=json.loads((folder/'live-session.json').read_text());initial=live_edits.parameters(doc);source=live_edits.digest(folder/'assembly.FCStd')
 def request(parameters,revision):
  identifier=uuid.uuid4().hex;queue=folder/'edit-queue';live_edits.write(queue/(identifier+'.request.json'),dict(parameters=parameters,expected_revision=revision))
  result=queue/(identifier+'.result.json');deadline=time.monotonic()+10
  while not result.exists() and time.monotonic()<deadline:app.processEvents();time.sleep(.01)
  return json.loads(result.read_text())
 assert 'Stale' in request({'Span':1850},'stale')['error']
 assert 'supported' in request({'Shape':'code'},state['revision'])['error']
 # A controlled GUI failure after a real recompute must roll back the parameters and dependent shape.
 assert request({'Span':1850},state['revision'])['status']=='failed'
 assert live_edits.parameters(doc)==initial and box.Length.Value==initial['Span']
 assert live_edits.digest(folder/'assembly.FCStd')==source
 assert json.loads((folder/'live-session.json').read_text())==state
 timer.stop()
print('PASS: stale revision, unsupported edit, native transaction rollback, dependent geometry restored, original file unchanged')
