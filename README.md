# Topos

> Treating programs as morphisms in a world of commodity code.

**Topos** classifies code quality using a **Heyting Algebra**—a lattice of evaluation values that expresses partial confidence about correctness and maintainability. Instead of a single score, every program is mapped to one of six stages:

| Symbol | Stage          | Meaning                            |
| ------ | -------------- | ---------------------------------- |
| ⊥      | `INVALID`      | Fails to parse                     |
| ○      | `HALLUCINATED` | Parses but logically vacuous       |
| ◑      | `NOISY`        | Syntactically valid but repetitive |
| ◒      | `WEAK`         | Functional with elevated risk      |
| ◐      | `COMMODITY`    | Functional but structurally weak   |
| ⊤      | `VERIFIED`     | Maintainable, human-aligned        |

This represents a our development of a **subobject classifier** (Ω), that for any piece of code X, a characteristic map χ: X → Ω combines cyclomatic complexity and entropy into an evaluation value in the lattice.

## Install

**Binary (fastest):**

```bash
curl -sSL https://raw.githubusercontent.com/Krv-Labs/topos/main/install.sh | bash
```

**From source:**

```bash
git clone https://github.com/Krv-Labs/topos.git
cd topos && uv pip install -e .
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

print(result.evaluation)        # ◐ COMMODITY
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
│   ├── policies.py    # Metric-to-lattice evaluation sections
│   └── omega.py       # The Subobject Classifier
├── metrics/
│   ├── complexity.py  # Cyclomatic complexity
│   ├── distance.py    # AST edit distance
│   └── entropy.py     # Kolmogorov proxy via compression
└── utils/
    └── tree_sitter.py # AST parsing
```
