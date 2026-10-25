"""Bounded native boolean features with persistent recompute validation."""
import math
import hashlib
from collections import OrderedDict
import native

TYPES = {'cut':'Part::Cut', 'fuse':'Part::Fuse', 'common':'Part::Common'}
VOLUME_TOLERANCE_MM3 = 1e-6


_CUT_PROOF_CACHE_LIMIT = 1024
_cut_proof_cache = OrderedDict()


def validate_cut_material(base, result):
    """Prove real removal and no added solid; curved-solid integrals can disagree.

    A positive scalar volume difference cannot establish containment either.
    Cache only successful exact-BRep proofs, never mutable shapes or identities.
    """
    for shape in (base, result):
        if (not native.shape_is_valid(shape) or not shape.Solids
                or not math.isfinite(shape.Volume)
                or shape.Volume <= VOLUME_TOLERANCE_MM3):
            raise ValueError('cut requires valid nonempty solid geometry')
    if len(result.Solids) != 1:
        raise ValueError('cut requires a single-solid result')
    key = tuple(hashlib.sha256(s.cleaned().exportBrepToString().encode()).digest()
                for s in (base, result))
    if key[0] == key[1]:
        raise ValueError('cut removes no measurable material')
    if key in _cut_proof_cache:
        _cut_proof_cache.move_to_end(key)
        return
    added = result.cut(base)
    if added.Solids:
        raise ValueError('cut adds material outside its base')
    removed = base.cut(result)
    if (not removed.Solids or not native.shape_is_valid(removed)
            or not math.isfinite(removed.Volume)
            or removed.Volume <= VOLUME_TOLERANCE_MM3):
        raise ValueError('cut removes no measurable material')
    _cut_proof_cache[key] = True
    while len(_cut_proof_cache) > _CUT_PROOF_CACHE_LIMIT:
        _cut_proof_cache.popitem(last=False)


def validate_cut_feature(obj):
    # Part::Cut builds from operand coordinates, then applies its own placement.
    # A legitimately placed final cut must be checked before that transform.
    result = obj.Shape.copy()
    result.Placement = obj.Placement.inverse().multiply(result.Placement)
    try:
        validate_cut_material(obj.Base.Shape, result)
    except ValueError as error:
        raise ValueError(f'{obj.Name}: {error}') from error


def _acyclic(roots):
    visited, active = set(), set()
    def visit(obj, depth):
        if obj.Name in active:raise ValueError('cyclic boolean dependency')
        if obj.Name in visited:return
        if depth > 64 or len(visited)+len(active) >= 512:
            raise ValueError('boolean dependency graph exceeds bounded scope')
        active.add(obj.Name)
        for child in obj.OutList:visit(child, depth+1)
        active.remove(obj.Name);visited.add(obj.Name)
    for obj in roots:visit(obj, 0)


def validate(obj, defer_material=False):
    """Re-run after edits/reopen; metadata never bypasses native shape checks."""
    if not getattr(obj, 'WrightTypedBoolean', False):return
    if obj.TypeId not in TYPES.values():raise ValueError('typed boolean has unexpected native type')
    _acyclic([obj])
    result, base, tool = obj.Shape, obj.Base.Shape, obj.Tool.Shape
    for shape in (result, base, tool):
        if not native.shape_is_valid(shape) or len(shape.Solids)!=1:
            raise ValueError('typed boolean requires valid single-solid operands and result')
        if not math.isfinite(shape.Volume) or shape.Volume<=VOLUME_TOLERANCE_MM3:
            raise ValueError('typed boolean has empty or nonfinite volume')
    if obj.TypeId=='Part::Cut' and not defer_material:
        validate_cut_feature(obj)
    if obj.TypeId=='Part::Fuse' and result.Volume-max(base.Volume,tool.Volume)<=VOLUME_TOLERANCE_MM3:
        raise ValueError('fuse adds no measurable material')


def create(doc, name, operation, base, tool, retain_tool_as_part=False):
    """Create one new editable feature; no replacement/rewiring or arbitrary code."""
    result_name=native.name(name)
    base_name=native.name(base);tool_name=native.name(tool)
    if not isinstance(operation,str) or operation not in TYPES:raise ValueError('operation must be cut, fuse or common')
    if not isinstance(retain_tool_as_part,bool):raise ValueError('retain_tool_as_part must be boolean')
    if retain_tool_as_part and operation!='cut':raise ValueError('only cuts can retain a mating tool as a physical part')
    if doc.getObject(result_name) is not None:raise ValueError('result name already exists')
    if result_name in (base_name,tool_name):raise ValueError('boolean result cannot reference itself')
    if base_name==tool_name:raise ValueError('boolean requires two distinct operands')
    operands=[doc.getObject(n) for n in (base_name,tool_name)]
    if any(o is None for o in operands):raise ValueError('unknown boolean operand')
    _acyclic(operands)
    for obj in operands:
        info=native.describe(obj)
        if info['solids']!=1 or info['assembly_container'] or getattr(obj,'NonphysicalKeepout',False):
            raise ValueError('boolean operands must be physical single solids')
    doc.openTransaction('Boolean '+operation+' '+result_name)
    try:
        from assembly_frames import document_operand
        operands=[document_operand(doc,obj,result_name[:70]+suffix)
                  for obj,suffix in zip(operands,('BaseFrame','ToolFrame'))]
        result=doc.addObject(TYPES[operation],result_name)
        result.Base,result.Tool=operands
        result.addProperty('App::PropertyBool','WrightTypedBoolean','Wright')
        result.WrightTypedBoolean=True
        result.setEditorMode('WrightTypedBoolean',1)
        result.addProperty('App::PropertyBool','WrightRetainToolAsPart','Wright')
        result.WrightRetainToolAsPart=retain_tool_as_part
        result.setEditorMode('WrightRetainToolAsPart',1)
        doc.recompute()
        native.validate_document(doc)
        validate(result)
        evidence=native.describe(result)
        doc.commitTransaction()
        return evidence
    except Exception:
        doc.abortTransaction();doc.recompute();raise
