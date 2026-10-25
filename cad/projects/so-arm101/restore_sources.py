"""Restore pinned FreeCAD benchmark inputs without checking CAD caches into Git."""
import argparse
import hashlib
import json
from pathlib import Path
import tempfile
from urllib.request import urlopen
from urllib.parse import unquote, quote

PROJECT = Path(__file__).resolve().parent
ROOT = PROJECT.parents[2]


def verify(data, entry):
    if len(data) != entry['bytes']:
        raise ValueError('Source byte count differs: ' + entry['upstream_path'])
    if hashlib.sha256(data).hexdigest() != entry['sha256']:
        raise ValueError('Source SHA256 differs: ' + entry['upstream_path'])
    blob = b'blob ' + str(len(data)).encode() + b'\0' + data
    if hashlib.sha1(blob).hexdigest() != entry['git_blob_sha1']:
        raise ValueError('Git blob SHA1 differs: ' + entry['upstream_path'])


def restore(bundle='articulation', check_only=False):
    source = json.loads((PROJECT / 'source-manifest.json').read_text())
    articulation = json.loads((PROJECT / 'articulation-source-manifest.json').read_text())
    manifests = [source, articulation] if bundle == 'all' else [articulation]
    entries = {f['path']: (m, f) for m in manifests for f in m['files']}
    for entry in source['files']:
        if entry['upstream_path'] in ('LICENSE', 'CITATION.cff'):
            entries[entry['path']] = (source, entry)
    for relative, (manifest, entry) in entries.items():
        target = (ROOT / relative).resolve()
        target.relative_to((PROJECT / 'upstream').resolve())
        prefix = 'https://raw.githubusercontent.com/TheRobotStudio/SO-ARM100/'
        expected_url = prefix + manifest['commit'] + '/' + entry['upstream_path']
        if unquote(entry['url']) != expected_url:
            raise ValueError('Unexpected source URL for ' + relative)
        if target.exists():
            verify(target.read_bytes(), entry)
            print('VERIFIED', relative, flush=True)
            continue
        if check_only:
            raise FileNotFoundError(target)
        with urlopen(quote(expected_url, safe=':/'), timeout=60) as response:
            data = response.read(entry['bytes'] + 1)
        verify(data, entry)
        target.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(dir=target.parent, delete=False) as tmp:
            temporary = Path(tmp.name)
            tmp.write(data)
        try:
            temporary.replace(target)
        finally:
            temporary.unlink(missing_ok=True)
        print('RESTORED', relative, flush=True)
    return len(entries)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle', choices=('articulation', 'all'), default='articulation')
    parser.add_argument('--check-only', action='store_true')
    args = parser.parse_args()
    print('Verified source files:', restore(args.bundle, args.check_only))
