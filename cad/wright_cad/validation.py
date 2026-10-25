"""Backend-independent validation. Contact never authorizes solid overlap."""
import math
import re


def finite_number(value, label, *, positive=False, nonnegative=False):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
        raise ValueError(f"{label} must be a finite number")
    if positive and value <= 0 or nonnegative and value < 0:
        raise ValueError(f"{label} is outside its physical range")
    return value


def identifier(value, label):
    if not isinstance(value, str) or not re.fullmatch(r"[A-Za-z][A-Za-z0-9_]*", value):
        raise ValueError(f"{label} must be an identifier")
    return value


def finite_tree(value, label):
    if isinstance(value, (list, tuple)):
        for i, child in enumerate(value):
            finite_tree(child, f"{label}[{i}]")
    elif isinstance(value, (int, float)) and not isinstance(value, bool):
        finite_number(value, label)


def validate_instances(instances):
    if not isinstance(instances, list) or not instances:
        raise ValueError("assembly must contain at least one instance")
    names = set()
    for inst in instances:
        name = identifier(inst["name"], "instance name")
        if name in names:
            raise ValueError(f"duplicate instance name {name!r}")
        placement = inst["placement"]
        if set(placement) == {"loc"}:
            for key in ("pos", "rot"):
                vector = placement["loc"][key]
                if not isinstance(vector, (list, tuple)) or len(vector) != 3:
                    raise ValueError(f"{name}.{key} must have three values")
                for value in vector:
                    finite_number(value, f"{name}.{key}")
        elif set(placement) == {"mate"}:
            mate = placement["mate"]
            if mate["to"] not in names:
                raise ValueError(f"{name}: mate target must be declared earlier")
            for key in ("mine", "theirs"):
                identifier(mate[key], f"{name}.{key}")
        else:
            raise ValueError(f"{name}: exactly one placement is required")
        names.add(name)
    for inst in instances:
        for other in inst.get("touches") or []:
            if other not in names or other == inst["name"]:
                raise ValueError(f"{inst['name']}: invalid contact target {other!r}")


def pair_result(a, b, volume, distance, expected_contact, volume_tol, contact_tol):
    """Classify independent kernel evidence; failed measurements must fail closed."""
    finite_number(volume, "intersection volume", nonnegative=True)
    finite_number(distance, "minimum distance", nonnegative=True)
    if volume > volume_tol:
        return "clash", dict(a=a, b=b, volume_mm3=volume)
    if expected_contact and distance > contact_tol:
        return "floating", dict(a=a, b=b, distance_mm=distance)
    return "contact" if expected_contact else "clearance", dict(a=a, b=b, clearance_mm=distance)
