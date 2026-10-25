"""Reproduce installed Rocket source edits from the pinned upstream files."""
import json,hashlib,subprocess,tempfile
from pathlib import Path
root=Path(__file__).resolve().parents[2]
manifest=json.loads((root/'cad/freecad/rocket-workbench.json').read_text())
workbench=Path(manifest['local_path'])
assert subprocess.check_output(['git','-C',str(workbench),'rev-parse','HEAD'],text=True).strip()==manifest['commit']
files=set()
with tempfile.TemporaryDirectory(prefix='wright-rocket-patch-check-') as tmp:
    folder=Path(tmp)
    for entry in manifest['patches']:
        patch=root/'cad/freecad'/entry['file']
        assert hashlib.sha256(patch.read_bytes()).hexdigest()==entry['sha256']
        for line in patch.read_text().splitlines():
            if line.startswith('--- a/'):
                relative=line[6:];target=folder/relative
                if relative not in files:
                    target.parent.mkdir(parents=True,exist_ok=True)
                    target.write_bytes(subprocess.check_output(['git','-C',str(workbench),'show',manifest['commit']+':'+relative]))
            if line.startswith('+++ b/'):files.add(line[6:])
        subprocess.run(['git','apply',str(patch)],cwd=folder,check=True,capture_output=True)
    for relative in sorted(files):
        assert (folder/relative).read_bytes()==(workbench/relative).read_bytes(),relative
        if relative.endswith('.py'):compile((folder/relative).read_text(),relative,'exec')
report=dict(passed=True,upstream_commit=manifest['commit'],patched_files=sorted(files),patches=manifest['patches'])
out=root/'out/freecad-openrocket-verified';out.mkdir(exist_ok=True)
(out/'patch-reproduction.json').write_text(json.dumps(report,indent=2))
print('Reproduced installed Rocket sources from',len(manifest['patches']),'patches across',len(files),'files')
