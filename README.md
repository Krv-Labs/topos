# Topos

> Treating programs as morphisms in a world of commodity code.

**Topos** classifies code quality using a **Heyting Algebra**—a lattice of truth values from topos theory that expresses partial truths about correctness and maintainability. Instead of a single score, every program is mapped to one of four stages:

| Symbol | Stage          | Meaning                          |
| ------ | -------------- | -------------------------------- |
| ⊥      | `INVALID`      | Fails to parse                   |
| ○      | `HALLUCINATED` | Parses but logically vacuous     |
| ◐      | `COMMODITY`    | Functional but structurally weak |
| ⊤      | `VERIFIED`     | Maintainable, human-aligned      |

The **subobject classifier** (Ω) from topos theory drives this: for any piece of code X, a characteristic map χ: X → Ω combines cyclomatic complexity and entropy into a truth value in the lattice.

## Install

```bash
uv add topos
# or
pip install topos
```

## Usage

```bash
topos evaluate src/ -r       # classify a directory
topos inspect module.py      # detailed metrics
topos compare a.py b.py      # AST edit distance
```

```python
from topos import ProgramMorphism, SubobjectClassifier

morphism = ProgramMorphism.from_file("my_code.py")
result = SubobjectClassifier().classify_detailed(morphism)

print(result.truth_value)       # ◐ COMMODITY
print(result.complexity_score)  # 0.65
print(result.entropy_score)     # 0.42
```

## Architecture

```
topos/
├── core/
│   ├── morphism.py    # Programs as arrows between states
│   └── object.py      # AST as a categorical object
├── logic/
│   ├── lattice.py     # Heyting Algebra (meet, join, implies, ¬)
│   └── omega.py       # The Subobject Classifier
├── metrics/
│   ├── complexity.py  # Cyclomatic complexity
│   ├── distance.py    # AST edit distance
│   └── entropy.py     # Kolmogorov proxy via compression
└── utils/
    └── tree_sitter.py # AST parsing
```
