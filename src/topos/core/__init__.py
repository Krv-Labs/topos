"""
Core Module
-----------
Defines the fundamental categorical structures: Objects and Morphisms.

In the category of Programs:
- Objects are Abstract Syntax Trees (the 'shape' of code)
- Morphisms are programs themselves (transformations between states)
"""

from topos.core.morphism import ProgramMorphism
from topos.core.object import ProgramObject

__all__ = ["ProgramMorphism", "ProgramObject"]
