"""A persistent CAD worker: JSON-RPC 2.0 over stdio, one request per line.

The Rust side (crates/wright-cad/src/worker.rs) keeps a pool of these alive,
so build123d/OCC import once per worker (seconds), not once per part.
Shape follows the Language Server Protocol's stdio JSON-RPC and the
JSON-RPC 2.0 spec: `{"jsonrpc":"2.0","id":N,"method":M,"params":{...}}` in,
`{"jsonrpc":"2.0","id":N,"result":...}` or `{"jsonrpc":"2.0","id":N,
"error":{"code":C,"message":M,"data":traceback}}` out. Newline-delimited
rather than LSP's Content-Length framing (JSON never contains a raw newline).

Methods: ping, describe {part}, list {domain?}, plan {domain, design},
build {part, params, key, cache, material?, density?, stl?},
assemble {name, instances, out_dir, ...}.

Anything printed by part code or OCC goes to stderr: fd 1 is re-pointed at
fd 2 and the protocol keeps a private dup of the original stdout.
"""
import json
import os
import sys
import traceback
from pathlib import Path

# protocol channel first, before anything can print
_proto = os.fdopen(os.dup(1), "w", buffering=1)
os.dup2(2, 1)
sys.stdout = sys.stderr

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))  # cad/

from wright_cad import kernel, library  # noqa: E402  (slow: imports build123d)


def plan(domain: str, design: dict) -> dict:
    from wright_cad.validation import identifier
    identifier(domain, "domain")
    path = library.PARTS_ROOT / domain / "assembly.py"
    if not path.exists():
        raise FileNotFoundError(f"no assembly for domain {domain!r}: {path}")
    import importlib.util
    spec = importlib.util.spec_from_file_location(f"wright_parts.{domain}.assembly_{path.stat().st_mtime_ns}", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    asm = mod.assembly(design)
    out = asm.to_json()
    for inst in out["instances"]:
        inst["params"] = library.check_params(inst["part"], inst["params"])  # defaults filled: the hash sees them
    return out


METHODS = {
    "ping": lambda: {"pid": os.getpid()},
    "describe": lambda part: library.describe(part),
    "list": lambda domain=None: [library.describe(p) for p in library.all_parts(domain)],
    "plan": plan,
    "build": kernel.build,
    "assemble": kernel.assemble,
}


def main():
    _proto.write(json.dumps({"jsonrpc": "2.0", "method": "ready", "params": {"pid": os.getpid()}}) + "\n")
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        rid = None
        try:
            req = json.loads(line, parse_constant=lambda value: (_ for _ in ()).throw(ValueError(f"invalid JSON constant {value}")))
        except ValueError as e:
            _proto.write(json.dumps({"jsonrpc": "2.0", "id": None, "error": {"code": -32700, "message": str(e)}}) + "\n")
            continue
        if not isinstance(req, dict) or req.get("jsonrpc") != "2.0" or not isinstance(req.get("method"), str):
            _proto.write(json.dumps({"jsonrpc": "2.0", "id": None, "error": {"code": -32600, "message": "invalid JSON-RPC request"}}) + "\n")
            continue
        rid = req.get("id")
        if rid is not None and (isinstance(rid, bool) or not isinstance(rid, (str, int))):
            _proto.write(json.dumps({"jsonrpc": "2.0", "id": None, "error": {"code": -32600, "message": "invalid request id"}}) + "\n")
            continue
        try:
            if req["method"] not in METHODS:
                msg = {"jsonrpc": "2.0", "id": rid, "error": {"code": -32601, "message": "unknown method"}}
            elif not isinstance(req.get("params", {}), dict):
                msg = {"jsonrpc": "2.0", "id": rid, "error": {"code": -32602, "message": "params must be an object"}}
            else:
                fn = METHODS[req["method"]]
                import inspect
                params = req.get("params", {})
                try:
                    inspect.signature(fn).bind(**params)
                except TypeError as e:
                    msg = {"jsonrpc": "2.0", "id": rid, "error": {"code": -32602, "message": str(e)}}
                else:
                    res = fn(**params)
                    msg = {"jsonrpc": "2.0", "id": rid, "result": res}
            encoded = json.dumps(msg, default=float, allow_nan=False)
        except Exception as e:
            encoded = json.dumps({"jsonrpc": "2.0", "id": rid, "error": {"code": -32000,
                                 "message": f"{type(e).__name__}: {e}", "data": traceback.format_exc()}})
        if "id" in req:  # JSON-RPC notifications have no response
            _proto.write(encoded + "\n")



if __name__ == "__main__":
    main()
