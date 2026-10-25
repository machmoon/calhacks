"""Explicit, unverified source-cut inspection with strict changed-cut validation.

No persisted proof claims are trusted. Checkpoint policy markers restore pending
status; only this process's successful exact proofs can discharge its ledger.
"""
import hashlib
import json
from collections import OrderedDict

MARKER = 'WrightCutProofPolicy'
POLICY = 'deferred-source-cuts-v1'
_ledgers = OrderedDict()
_MAX_DOCUMENTS = 16


def _shape_hash(shape):
    return hashlib.sha256(shape.cleaned().exportBrepToString().encode()).hexdigest()


def fingerprint(obj):
    values = [obj.TypeId, obj.Name, obj.Base.Name, obj.Tool.Name,
              bool(getattr(obj, 'Refine', False)),
              bool(getattr(obj, 'WrightTypedBoolean', False)),
              _shape_hash(obj.Base.Shape), _shape_hash(obj.Tool.Shape),
              _shape_hash(obj.Shape)]
    return hashlib.sha256(json.dumps(values, separators=(',', ':')).encode()).hexdigest()


def _ledger(doc):
    entry = _ledgers.get(id(doc))
    return entry if entry is not None and entry['document'] is doc else None


def has_marker(doc):
    return MARKER in doc.Meta


def defer_source(doc):
    cuts = [o for o in doc.Objects if o.TypeId == 'Part::Cut']
    if len(cuts) > 4096:
        raise ValueError('deferred source cut count exceeds 4096')
    _ledgers[id(doc)] = dict(document=doc,
        source={o.Name: (o, fingerprint(o)) for o in cuts}, verified=set())
    _ledgers.move_to_end(id(doc))
    while len(_ledgers) > _MAX_DOCUMENTS:
        _ledgers.popitem(last=False)
    persist_policy(doc)


def clear(doc):
    _ledgers.pop(id(doc), None)


def should_defer(obj):
    ledger = _ledger(obj.Document)
    if ledger is None:
        return False
    fp = fingerprint(obj)
    if fp in ledger['verified']:
        return True  # This exact feature was already strictly proved.
    source = ledger['source'].get(obj.Name)
    return source is not None and source[0] is obj and source[1] == fp


def record_verified(obj):
    ledger = _ledger(obj.Document)
    if ledger is not None:
        ledger['verified'].add(fingerprint(obj))


def status(doc):
    ledger = _ledger(doc)
    if ledger is None:
        # A marker without its ephemeral ledger can never become a green state.
        if has_marker(doc):
            return dict(status='unverified', geometry_verified=False,
                pending_count=None, pending=[], reason='Deferred policy has no active ledger; reopen explicitly in deferred mode')
        return dict(status='strict', geometry_verified=True, pending_count=0, pending=[])
    pending = []
    for obj in doc.Objects:
        if obj.TypeId != 'Part::Cut':
            continue
        fp = fingerprint(obj)
        if fp not in ledger['verified']:
            source = ledger['source'].get(obj.Name)
            pending.append(dict(name=obj.Name, fingerprint=fp,
                unchanged_source=source is not None and source[0] is obj and source[1] == fp))
    return dict(status='unverified' if pending else 'verified',
        geometry_verified=not pending, pending_count=len(pending), pending=pending,
        scope='Cut material proofs only; separate fit/contact checks are still required')


def persist_policy(doc):
    info = status(doc)
    meta = dict(doc.Meta)
    if not info['geometry_verified']:
        meta[MARKER] = POLICY
    else:
        meta.pop(MARKER, None)
    doc.Meta = meta
    return info


def require_complete(doc):
    info = status(doc)
    if not info['geometry_verified']:
        raise ValueError('checked save blocked: source cut proofs remain pending; use save_preview/checkpoint for explicitly unverified geometry')
    return info
