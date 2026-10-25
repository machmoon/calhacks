"""Wright's CAD library: parametric build123d parts on Open CASCADE.

See docs/ARCHITECTURE.md "CAD" for the contract; `library.py` for `@part`
and `Param`, `assembly.py` for `Assembly`, `Loc`, `Mate`.
"""
from .assembly import Assembly, Loc, Mate
from .library import Param, part
from .materials import MATERIALS

__all__ = ["Assembly", "Loc", "Mate", "Param", "part", "MATERIALS"]
