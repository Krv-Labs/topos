"""
Logic Module
------------
Implements the internal logic of our Topos: a Heyting Algebra.

Unlike classical Boolean logic, intuitionistic logic allows for
'degrees of evaluation'—perfect for evaluating code that may be
functionally correct but structurally unsound.
"""

from topos.logic.lattice import (
    EvaluationLattice,
    EvaluationValue,
)
from topos.logic.omega import SubobjectClassifier

__all__ = [
    "EvaluationLattice",
    "EvaluationValue",
    "SubobjectClassifier",
]
