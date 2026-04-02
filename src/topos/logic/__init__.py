"""
Logic Module
------------
Implements the internal logic of our Topos: a Heyting Algebra.

Unlike classical Boolean logic, intuitionistic logic allows for 
'degrees of truth'—perfect for evaluating code that may be 
functionally correct but structurally unsound.
"""

from topos.logic.lattice import TruthLattice, TruthValue
from topos.logic.omega import SubobjectClassifier

__all__ = ["TruthLattice", "TruthValue", "SubobjectClassifier"]
