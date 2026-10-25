import tempfile
import unittest
import sys
from pathlib import Path
from run import resolve_brief, check_outcomes, build_with_progress, audit_with_progress

class BriefBoundary(unittest.TestCase):
    def test_builder_failure_is_preserved(self):
        with tempfile.TemporaryDirectory() as directory:
            output=Path(directory)
            code=build_with_progress([sys.executable,'-c','import sys; print("native failed"); sys.exit(7)'],output,lambda _:None)
            self.assertEqual(code,7)
            self.assertIn('native failed',(output/'freecad.log').read_text())
    def test_render_published_once_only_after_png_complete(self):
        import base64
        png=base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a6QAAAABJRU5ErkJggg==')
        with tempfile.TemporaryDirectory() as directory:
            output=Path(directory);(output/'cad').mkdir();image=output/'cad/assembled.png';seen=[]
            code="import time; from pathlib import Path; p=Path(%r); data=%r; p.write_bytes(data[:20]); time.sleep(1.2); p.write_bytes(data); time.sleep(1.2)" % (str(image),png)
            result=build_with_progress([sys.executable,'-c',code],output,lambda _:None,on_render=lambda path:seen.append((path,path.stat().st_size)))
            self.assertEqual(result,0);self.assertEqual(seen,[(image,len(png))])
    def test_audit_missing_and_failure_never_render_as_pass(self):
        rows = check_outcomes({'gates': {s: {'status': s, 'requirement': s} for s in ['passed','failed','pending','inconclusive']}})
        self.assertEqual(rows[0]['evidence']['Margin']['value'],1)
        self.assertEqual(rows[1]['evidence']['Margin']['value'],-1)
        self.assertIn('Missing',rows[2]['evidence'])
        self.assertIn('Missing',rows[3]['evidence'])
        with self.assertRaises(ValueError):
            check_outcomes({'gates': {'x': {'status':'skipped','requirement':'x'}}})
    def test_audit_progress_does_not_confuse_execution_with_verdict(self):
        with tempfile.TemporaryDirectory() as directory:
            output=Path(directory);(output/'audit').mkdir();path=output/'audit/probe-timings.json';messages=[]
            code="import json,time;from pathlib import Path;p=Path(%r);r={'stage':'geometry','status':'running','started_monotonic_seconds':1};p.write_text(json.dumps({'probes':[r]}));time.sleep(1.2);r.update(status='complete',duration_seconds=1.2,engineering_passed=False);p.write_text(json.dumps({'probes':[r]}))" % str(path)
            result=audit_with_progress([sys.executable,'-c',code],output,messages.append)
            self.assertEqual(result.returncode,0);self.assertEqual(len(messages),2);self.assertIn('started',messages[0]);self.assertIn('execution complete',messages[1]);self.assertIn('local result did not pass',messages[1])
    def test_audit_timeout_preserves_failure(self):
        import subprocess
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(subprocess.TimeoutExpired):audit_with_progress([sys.executable,'-c','import time;time.sleep(30)'],Path(directory),lambda _:None,timeout_seconds=.05)
    @unittest.skipUnless(sys.platform!='win32','POSIX process-group cancellation')
    def test_parent_sigterm_stops_owned_audit_and_probe(self):
        import json,os,signal,subprocess,time
        with tempfile.TemporaryDirectory() as directory:
            output=Path(directory);audit=output/'audit_child.py';controller=output/'controller.py';pids=output/'pids.json'
            audit.write_text("import os,sys,json,subprocess; from pathlib import Path; p=subprocess.Popen([sys.executable,'-c','import time;time.sleep(60)',sys.argv[1]]); Path(sys.argv[1]+'/pids.json').write_text(json.dumps([os.getpid(),p.pid])); p.wait()")
            controller.write_text("import sys,signal;from pathlib import Path;sys.path.insert(0,%r);from run import audit_with_progress;old=signal.getsignal(signal.SIGTERM)\ntry:audit_with_progress([sys.executable,%r,%r],Path(%r),lambda _:None)\nfinally:Path(%r).write_text(str(signal.getsignal(signal.SIGTERM)==old))" % (str(Path(__file__).parent),str(audit),str(output),str(output),str(output/'restored')))
            parent=subprocess.Popen([sys.executable,str(controller)],start_new_session=True);owned=[]
            try:
                deadline=time.monotonic()+5
                while not pids.exists() and time.monotonic()<deadline:time.sleep(.02)
                self.assertTrue(pids.exists());owned=json.loads(pids.read_text());os.kill(parent.pid,signal.SIGTERM);self.assertEqual(parent.wait(timeout=5),143);self.assertEqual((output/'restored').read_text(),'True')
                for pid in owned:
                    deadline=time.monotonic()+3
                    while time.monotonic()<deadline:
                        status=subprocess.run(['ps','-o','stat=','-p',str(pid)],capture_output=True,text=True).stdout.strip()
                        if not status or status.startswith('Z'):break
                        time.sleep(.02)
                    self.assertTrue(not status or status.startswith('Z'),f'Owned process {pid} survived cancellation')
            finally:
                if parent.poll() is None:parent.kill();parent.wait()
                for pid in owned:
                    command=subprocess.run(['ps','-o','command=','-p',str(pid)],capture_output=True,text=True).stdout
                    if str(output) in command:
                        try:os.kill(pid,signal.SIGKILL)
                        except ProcessLookupError:pass
    def test_native_snapshot_callback_precedes_render_completion_and_keeps_short_phases(self):
        import json
        with tempfile.TemporaryDirectory() as directory:
            output=Path(directory);(output/'cad').mkdir();messages=[];ready=[]
            code="import json,time;from pathlib import Path;p=Path(%r);(p/'cad/audit-ready.json').write_text('{}');(p/'cad/build-phases.jsonl').write_text(json.dumps({'phase':'STEP; export','monotonic_seconds':1})+'\\n'+json.dumps({'phase':'render','monotonic_seconds':1.125})+'\\n');time.sleep(1.5);(p/'render-finished').write_text('yes')" % str(output)
            def snapshot(path):ready.append(not (output/'render-finished').exists())
            result=build_with_progress([sys.executable,'-c',code],output,messages.append,on_ready=snapshot)
            self.assertEqual(result,0);self.assertEqual(ready,[True])
            self.assertTrue(any('STEP; export (0.125s)' in m for m in messages))

    def test_reads_existing_project_and_rejects_escape(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            brief=root/'cad/projects/example/BUILD_PROMPT.txt'
            brief.parent.mkdir(parents=True);brief.write_text('Build native CAD')
            self.assertEqual(resolve_brief('cad/projects/example/BUILD_PROMPT.txt',root),brief.resolve())
            for path in ('../BUILD_PROMPT.txt','cad/projects/missing/BUILD_PROMPT.txt','cad/projects/example/other.txt'):
                with self.assertRaises(ValueError):resolve_brief(path,root)
    def test_symlink_cannot_escape_project_tree(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);(root/'cad/projects').mkdir(parents=True)
            outside=root/'outside';outside.mkdir();(outside/'BUILD_PROMPT.txt').write_text('x')
            (root/'cad/projects/link').symlink_to(outside)
            with self.assertRaises(ValueError):resolve_brief('cad/projects/link/BUILD_PROMPT.txt',root)

if __name__=='__main__':unittest.main()

class CadReadiness(unittest.TestCase):
    def test_readiness_requires_every_view_and_matching_native(self):
        import json,hashlib
        from run import cad_ready
        with tempfile.TemporaryDirectory() as directory:
            p=Path(directory);(p/'assembly.FCStd').write_bytes(b'native fixture')
            for name in ['airframe.step','open.FCMacro','proxy.py']:(p/name).write_text('fixture')
            for view in ['assembled','internals','rear-three-quarter','rear','cutaway','exploded']:
                (p/(view+'.png')).write_bytes(b'\x89PNG\r\n\x1a\nfixture')
            manifest=dict(native_sha256=hashlib.sha256((p/'assembly.FCStd').read_bytes()).hexdigest(),invalid_physical=[],proxy_module='proxy.py')
            (p/'artifact-manifest.json').write_text(json.dumps(manifest))
            self.assertTrue(cad_ready(p,119)['under_two_minutes'])
            self.assertFalse(cad_ready(p,120)['under_two_minutes'])
            self.assertFalse(cad_ready(p,119)['engineering_verified'])
            (p/'exploded.png').unlink()
            with self.assertRaises(FileNotFoundError):cad_ready(p,119)
            (p/'assembly.FCStd').write_bytes(b'changed')
            with self.assertRaises(ValueError):cad_ready(p,119)
