"""Characterization grid pinning scorer behavior across every gate bound.

Generated from the pre-gate-spec scorers (see the gates.py unification): each
case pins the exact ``(score, achieved, interpretation)`` triple so the
refactor that moves gate comparisons into ``topos.evaluation.policies.gates``
is provably verdict-, score-, and prose-preserving. Regenerate only when a
calibration constant deliberately changes.

Cases straddle every bound: cyclomatic {0, 14.9, 15, 15.1, 40}, entropy
{0, 0.19, 0.2, 0.5, 0.8, 0.81} x entrypoint, max_function_complexity
{9.9, 10, 10.1, 20}, instability {0.29, 0.3, 0.7, 0.71, 0.94, 0.95, 1.0}
x fan_in {None, 0, 1, 16} x entrypoint (the fan_in=None row pins that an
unmeasured fan-in never grants the entrypoint exemption), fan_out
{14, 15, 16}, and secure counts {0, 1, 3}.
"""

# ruff: noqa: E501 — generated literal table; long lines are inherent

from __future__ import annotations

import pytest
from topos.evaluation.policies.composable import score_coupling
from topos.evaluation.policies.secure import score_secure
from topos.evaluation.policies.simple import score_simple

_SCORERS = {
    "simple": score_simple,
    "composable": score_coupling,
    "secure": score_secure,
}

# (pillar, kwargs, expected_score, expected_achieved, expected_interpretation)
CASES = [
    (
        "simple",
        {"cyclomatic": 0.0},
        1.0,
        True,
        {"cfg.cyclomatic": "cyclomatic complexity (0) within threshold (<= 15.0)"},
    ),
    (
        "simple",
        {"cyclomatic": 14.9},
        0.6275,
        True,
        {"cfg.cyclomatic": "cyclomatic complexity (15) within threshold (<= 15.0)"},
    ),
    (
        "simple",
        {"cyclomatic": 15.0},
        0.625,
        True,
        {"cfg.cyclomatic": "cyclomatic complexity (15) within threshold (<= 15.0)"},
    ),
    (
        "simple",
        {"cyclomatic": 15.1},
        0.6225,
        False,
        {"cfg.cyclomatic": "cyclomatic complexity (15) exceeds threshold (> 15.0)"},
    ),
    (
        "simple",
        {"cyclomatic": 40.0},
        0.0,
        False,
        {"cfg.cyclomatic": "cyclomatic complexity (40) exceeds threshold (> 15.0)"},
    ),
    (
        "simple",
        {"entropy": 0.0, "is_entrypoint_module": False},
        0.0,
        False,
        {"ast.entropy": "entropy (0.00) is too low; code may be repetitive or trivial"},
    ),
    (
        "simple",
        {"entropy": 0.0, "is_entrypoint_module": True},
        0.0,
        True,
        {
            "ast.entropy": "entropy (0.00) is low, but tolerated for import/export-only entrypoint modules"
        },
    ),
    (
        "simple",
        {"entropy": 0.19, "is_entrypoint_module": False},
        0.38,
        False,
        {"ast.entropy": "entropy (0.19) is too low; code may be repetitive or trivial"},
    ),
    (
        "simple",
        {"entropy": 0.19, "is_entrypoint_module": True},
        0.38,
        True,
        {
            "ast.entropy": "entropy (0.19) is low, but tolerated for import/export-only entrypoint modules"
        },
    ),
    (
        "simple",
        {"entropy": 0.2, "is_entrypoint_module": False},
        0.4,
        True,
        {"ast.entropy": "entropy (0.20) within structured range [0.2, 0.8]"},
    ),
    (
        "simple",
        {"entropy": 0.2, "is_entrypoint_module": True},
        0.4,
        True,
        {"ast.entropy": "entropy (0.20) within structured range [0.2, 0.8]"},
    ),
    (
        "simple",
        {"entropy": 0.5, "is_entrypoint_module": False},
        1.0,
        True,
        {"ast.entropy": "entropy (0.50) within structured range [0.2, 0.8]"},
    ),
    (
        "simple",
        {"entropy": 0.5, "is_entrypoint_module": True},
        1.0,
        True,
        {"ast.entropy": "entropy (0.50) within structured range [0.2, 0.8]"},
    ),
    (
        "simple",
        {"entropy": 0.8, "is_entrypoint_module": False},
        0.3999999999999999,
        True,
        {"ast.entropy": "entropy (0.80) within structured range [0.2, 0.8]"},
    ),
    (
        "simple",
        {"entropy": 0.8, "is_entrypoint_module": True},
        0.3999999999999999,
        True,
        {"ast.entropy": "entropy (0.80) within structured range [0.2, 0.8]"},
    ),
    (
        "simple",
        {"entropy": 0.81, "is_entrypoint_module": False},
        0.3799999999999999,
        False,
        {"ast.entropy": "entropy (0.81) is too high; code may be unstructured"},
    ),
    (
        "simple",
        {"entropy": 0.81, "is_entrypoint_module": True},
        0.3799999999999999,
        False,
        {"ast.entropy": "entropy (0.81) is too high; code may be unstructured"},
    ),
    (
        "simple",
        {"max_function_complexity": 9.9},
        0.505,
        True,
        {
            "ast.max_function_complexity": "max function complexity (10) within threshold (<= 10.0)"
        },
    ),
    (
        "simple",
        {"max_function_complexity": 10.0},
        0.5,
        True,
        {
            "ast.max_function_complexity": "max function complexity (10) within threshold (<= 10.0)"
        },
    ),
    (
        "simple",
        {"max_function_complexity": 10.1},
        0.495,
        False,
        {
            "ast.max_function_complexity": "max function complexity (10) exceeds threshold (> 10.0)"
        },
    ),
    (
        "simple",
        {"max_function_complexity": 20.0},
        0.0,
        False,
        {
            "ast.max_function_complexity": "max function complexity (20) exceeds threshold (> 10.0)"
        },
    ),
    (
        "simple",
        {"cyclomatic": 16.0, "entropy": 0.5, "max_function_complexity": 5.0},
        0.6,
        False,
        {
            "cfg.cyclomatic": "cyclomatic complexity (16) exceeds threshold (> 15.0)",
            "ast.entropy": "entropy (0.50) within structured range [0.2, 0.8]",
            "ast.max_function_complexity": "max function complexity (5) within threshold (<= 10.0)",
        },
    ),
    (
        "simple",
        {
            "cyclomatic": 3.0,
            "entropy": 0.1,
            "max_function_complexity": 2.0,
            "is_entrypoint_module": True,
        },
        0.19999999999999996,
        True,
        {
            "cfg.cyclomatic": "cyclomatic complexity (3) within threshold (<= 15.0)",
            "ast.entropy": "entropy (0.10) is low, but tolerated for import/export-only entrypoint modules",
            "ast.max_function_complexity": "max function complexity (2) within threshold (<= 10.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.29, "fan_in": None, "is_entrypoint_module": False},
        0.9666666666666667,
        False,
        {"mdg.instability": "instability (0.29) is too low (module is too stable)"},
    ),
    (
        "composable",
        {"instability": 0.29, "fan_in": None, "is_entrypoint_module": True},
        0.9666666666666667,
        False,
        {"mdg.instability": "instability (0.29) is too low (module is too stable)"},
    ),
    (
        "composable",
        {"instability": 0.29, "fan_in": 0.0, "is_entrypoint_module": False},
        0.9666666666666667,
        False,
        {
            "mdg.instability": "instability (0.29) is too low (module is too stable)",
            "mdg.fan_in": "fan-in (0) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.29, "fan_in": 0.0, "is_entrypoint_module": True},
        0.9666666666666667,
        False,
        {
            "mdg.instability": "instability (0.29) is too low (module is too stable)",
            "mdg.fan_in": "fan-in (0) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.29, "fan_in": 1.0, "is_entrypoint_module": False},
        0.9666666666666667,
        False,
        {
            "mdg.instability": "instability (0.29) is too low (module is too stable)",
            "mdg.fan_in": "fan-in (1) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.29, "fan_in": 1.0, "is_entrypoint_module": True},
        0.9666666666666667,
        False,
        {
            "mdg.instability": "instability (0.29) is too low (module is too stable)",
            "mdg.fan_in": "fan-in (1) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.29, "fan_in": 16.0, "is_entrypoint_module": False},
        0.6,
        False,
        {
            "mdg.instability": "instability (0.29) is too low (module is too stable)",
            "mdg.fan_in": "fan-in (16) exceeds threshold (> 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.29, "fan_in": 16.0, "is_entrypoint_module": True},
        0.6,
        False,
        {
            "mdg.instability": "instability (0.29) is too low (module is too stable)",
            "mdg.fan_in": "fan-in (16) exceeds threshold (> 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.3, "fan_in": None, "is_entrypoint_module": False},
        1.0,
        True,
        {"mdg.instability": "instability (0.30) within balanced range [0.3, 0.7]"},
    ),
    (
        "composable",
        {"instability": 0.3, "fan_in": None, "is_entrypoint_module": True},
        1.0,
        True,
        {"mdg.instability": "instability (0.30) within balanced range [0.3, 0.7]"},
    ),
    (
        "composable",
        {"instability": 0.3, "fan_in": 0.0, "is_entrypoint_module": False},
        1.0,
        True,
        {
            "mdg.instability": "instability (0.30) within balanced range [0.3, 0.7]",
            "mdg.fan_in": "fan-in (0) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.3, "fan_in": 0.0, "is_entrypoint_module": True},
        1.0,
        True,
        {
            "mdg.instability": "instability (0.30) within balanced range [0.3, 0.7]",
            "mdg.fan_in": "fan-in (0) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.3, "fan_in": 1.0, "is_entrypoint_module": False},
        0.975,
        True,
        {
            "mdg.instability": "instability (0.30) within balanced range [0.3, 0.7]",
            "mdg.fan_in": "fan-in (1) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.3, "fan_in": 1.0, "is_entrypoint_module": True},
        0.975,
        True,
        {
            "mdg.instability": "instability (0.30) within balanced range [0.3, 0.7]",
            "mdg.fan_in": "fan-in (1) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.3, "fan_in": 16.0, "is_entrypoint_module": False},
        0.6,
        False,
        {
            "mdg.instability": "instability (0.30) within balanced range [0.3, 0.7]",
            "mdg.fan_in": "fan-in (16) exceeds threshold (> 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.3, "fan_in": 16.0, "is_entrypoint_module": True},
        0.6,
        False,
        {
            "mdg.instability": "instability (0.30) within balanced range [0.3, 0.7]",
            "mdg.fan_in": "fan-in (16) exceeds threshold (> 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.7, "fan_in": None, "is_entrypoint_module": False},
        1.0,
        True,
        {"mdg.instability": "instability (0.70) within balanced range [0.3, 0.7]"},
    ),
    (
        "composable",
        {"instability": 0.7, "fan_in": None, "is_entrypoint_module": True},
        1.0,
        True,
        {"mdg.instability": "instability (0.70) within balanced range [0.3, 0.7]"},
    ),
    (
        "composable",
        {"instability": 0.7, "fan_in": 0.0, "is_entrypoint_module": False},
        1.0,
        True,
        {
            "mdg.instability": "instability (0.70) within balanced range [0.3, 0.7]",
            "mdg.fan_in": "fan-in (0) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.7, "fan_in": 0.0, "is_entrypoint_module": True},
        1.0,
        True,
        {
            "mdg.instability": "instability (0.70) within balanced range [0.3, 0.7]",
            "mdg.fan_in": "fan-in (0) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.7, "fan_in": 1.0, "is_entrypoint_module": False},
        0.975,
        True,
        {
            "mdg.instability": "instability (0.70) within balanced range [0.3, 0.7]",
            "mdg.fan_in": "fan-in (1) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.7, "fan_in": 1.0, "is_entrypoint_module": True},
        0.975,
        True,
        {
            "mdg.instability": "instability (0.70) within balanced range [0.3, 0.7]",
            "mdg.fan_in": "fan-in (1) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.7, "fan_in": 16.0, "is_entrypoint_module": False},
        0.6,
        False,
        {
            "mdg.instability": "instability (0.70) within balanced range [0.3, 0.7]",
            "mdg.fan_in": "fan-in (16) exceeds threshold (> 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.7, "fan_in": 16.0, "is_entrypoint_module": True},
        0.6,
        False,
        {
            "mdg.instability": "instability (0.70) within balanced range [0.3, 0.7]",
            "mdg.fan_in": "fan-in (16) exceeds threshold (> 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.71, "fan_in": None, "is_entrypoint_module": False},
        0.9666666666666667,
        False,
        {
            "mdg.instability": "instability (0.71) is too high (module depends on too many things)"
        },
    ),
    (
        "composable",
        {"instability": 0.71, "fan_in": None, "is_entrypoint_module": True},
        0.9666666666666667,
        False,
        {
            "mdg.instability": "instability (0.71) is too high (module depends on too many things)"
        },
    ),
    (
        "composable",
        {"instability": 0.71, "fan_in": 0.0, "is_entrypoint_module": False},
        0.9666666666666667,
        False,
        {
            "mdg.instability": "instability (0.71) is too high (module depends on too many things)",
            "mdg.fan_in": "fan-in (0) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.71, "fan_in": 0.0, "is_entrypoint_module": True},
        0.9666666666666667,
        False,
        {
            "mdg.instability": "instability (0.71) is too high (module depends on too many things)",
            "mdg.fan_in": "fan-in (0) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.71, "fan_in": 1.0, "is_entrypoint_module": False},
        0.9666666666666667,
        False,
        {
            "mdg.instability": "instability (0.71) is too high (module depends on too many things)",
            "mdg.fan_in": "fan-in (1) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.71, "fan_in": 1.0, "is_entrypoint_module": True},
        0.9666666666666667,
        False,
        {
            "mdg.instability": "instability (0.71) is too high (module depends on too many things)",
            "mdg.fan_in": "fan-in (1) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.71, "fan_in": 16.0, "is_entrypoint_module": False},
        0.6,
        False,
        {
            "mdg.instability": "instability (0.71) is too high (module depends on too many things)",
            "mdg.fan_in": "fan-in (16) exceeds threshold (> 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.71, "fan_in": 16.0, "is_entrypoint_module": True},
        0.6,
        False,
        {
            "mdg.instability": "instability (0.71) is too high (module depends on too many things)",
            "mdg.fan_in": "fan-in (16) exceeds threshold (> 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.94, "fan_in": None, "is_entrypoint_module": False},
        0.20000000000000015,
        False,
        {
            "mdg.instability": "instability (0.94) is too high (module depends on too many things)"
        },
    ),
    (
        "composable",
        {"instability": 0.94, "fan_in": None, "is_entrypoint_module": True},
        0.20000000000000015,
        False,
        {
            "mdg.instability": "instability (0.94) is too high (module depends on too many things)"
        },
    ),
    (
        "composable",
        {"instability": 0.94, "fan_in": 0.0, "is_entrypoint_module": False},
        0.20000000000000015,
        False,
        {
            "mdg.instability": "instability (0.94) is too high (module depends on too many things)",
            "mdg.fan_in": "fan-in (0) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.94, "fan_in": 0.0, "is_entrypoint_module": True},
        0.20000000000000015,
        False,
        {
            "mdg.instability": "instability (0.94) is too high (module depends on too many things)",
            "mdg.fan_in": "fan-in (0) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.94, "fan_in": 1.0, "is_entrypoint_module": False},
        0.20000000000000015,
        False,
        {
            "mdg.instability": "instability (0.94) is too high (module depends on too many things)",
            "mdg.fan_in": "fan-in (1) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.94, "fan_in": 1.0, "is_entrypoint_module": True},
        0.20000000000000015,
        False,
        {
            "mdg.instability": "instability (0.94) is too high (module depends on too many things)",
            "mdg.fan_in": "fan-in (1) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.94, "fan_in": 16.0, "is_entrypoint_module": False},
        0.20000000000000015,
        False,
        {
            "mdg.instability": "instability (0.94) is too high (module depends on too many things)",
            "mdg.fan_in": "fan-in (16) exceeds threshold (> 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.94, "fan_in": 16.0, "is_entrypoint_module": True},
        0.20000000000000015,
        False,
        {
            "mdg.instability": "instability (0.94) is too high (module depends on too many things)",
            "mdg.fan_in": "fan-in (16) exceeds threshold (> 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.95, "fan_in": None, "is_entrypoint_module": False},
        0.1666666666666668,
        False,
        {
            "mdg.instability": "instability (0.95) is too high (module depends on too many things)"
        },
    ),
    (
        "composable",
        {"instability": 0.95, "fan_in": None, "is_entrypoint_module": True},
        0.1666666666666668,
        False,
        {
            "mdg.instability": "instability (0.95) is too high (module depends on too many things)"
        },
    ),
    (
        "composable",
        {"instability": 0.95, "fan_in": 0.0, "is_entrypoint_module": False},
        0.1666666666666668,
        False,
        {
            "mdg.instability": "instability (0.95) is too high (module depends on too many things)",
            "mdg.fan_in": "fan-in (0) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.95, "fan_in": 0.0, "is_entrypoint_module": True},
        0.1666666666666668,
        True,
        {
            "mdg.instability": "instability (0.95) is high, but tolerated for import/export-only entrypoint modules",
            "mdg.fan_in": "fan-in (0) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.95, "fan_in": 1.0, "is_entrypoint_module": False},
        0.1666666666666668,
        False,
        {
            "mdg.instability": "instability (0.95) is too high (module depends on too many things)",
            "mdg.fan_in": "fan-in (1) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.95, "fan_in": 1.0, "is_entrypoint_module": True},
        0.1666666666666668,
        False,
        {
            "mdg.instability": "instability (0.95) is too high (module depends on too many things)",
            "mdg.fan_in": "fan-in (1) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.95, "fan_in": 16.0, "is_entrypoint_module": False},
        0.1666666666666668,
        False,
        {
            "mdg.instability": "instability (0.95) is too high (module depends on too many things)",
            "mdg.fan_in": "fan-in (16) exceeds threshold (> 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 0.95, "fan_in": 16.0, "is_entrypoint_module": True},
        0.1666666666666668,
        False,
        {
            "mdg.instability": "instability (0.95) is too high (module depends on too many things)",
            "mdg.fan_in": "fan-in (16) exceeds threshold (> 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 1.0, "fan_in": None, "is_entrypoint_module": False},
        0.0,
        False,
        {
            "mdg.instability": "instability (1.00) is too high (module depends on too many things)"
        },
    ),
    (
        "composable",
        {"instability": 1.0, "fan_in": None, "is_entrypoint_module": True},
        0.0,
        False,
        {
            "mdg.instability": "instability (1.00) is too high (module depends on too many things)"
        },
    ),
    (
        "composable",
        {"instability": 1.0, "fan_in": 0.0, "is_entrypoint_module": False},
        0.0,
        False,
        {
            "mdg.instability": "instability (1.00) is too high (module depends on too many things)",
            "mdg.fan_in": "fan-in (0) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 1.0, "fan_in": 0.0, "is_entrypoint_module": True},
        0.0,
        True,
        {
            "mdg.instability": "instability (1.00) is high, but tolerated for import/export-only entrypoint modules",
            "mdg.fan_in": "fan-in (0) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 1.0, "fan_in": 1.0, "is_entrypoint_module": False},
        0.0,
        False,
        {
            "mdg.instability": "instability (1.00) is too high (module depends on too many things)",
            "mdg.fan_in": "fan-in (1) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 1.0, "fan_in": 1.0, "is_entrypoint_module": True},
        0.0,
        False,
        {
            "mdg.instability": "instability (1.00) is too high (module depends on too many things)",
            "mdg.fan_in": "fan-in (1) within threshold (<= 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 1.0, "fan_in": 16.0, "is_entrypoint_module": False},
        0.0,
        False,
        {
            "mdg.instability": "instability (1.00) is too high (module depends on too many things)",
            "mdg.fan_in": "fan-in (16) exceeds threshold (> 15.0)",
        },
    ),
    (
        "composable",
        {"instability": 1.0, "fan_in": 16.0, "is_entrypoint_module": True},
        0.0,
        False,
        {
            "mdg.instability": "instability (1.00) is too high (module depends on too many things)",
            "mdg.fan_in": "fan-in (16) exceeds threshold (> 15.0)",
        },
    ),
    (
        "composable",
        {"fan_out": 14.0},
        0.65,
        True,
        {"mdg.fan_out": "fan-out (14) within threshold (<= 15.0)"},
    ),
    (
        "composable",
        {"fan_out": 15.0},
        0.625,
        True,
        {"mdg.fan_out": "fan-out (15) within threshold (<= 15.0)"},
    ),
    (
        "composable",
        {"fan_out": 16.0},
        0.6,
        False,
        {"mdg.fan_out": "fan-out (16) exceeds threshold (> 15.0)"},
    ),
    (
        "secure",
        {"dangerous_calls": 0.0},
        1.0,
        True,
        {"cpg.dangerous_calls": "no reachable dangerous-API calls (0 <= 0.0)"},
    ),
    (
        "secure",
        {"dangerous_calls": 1.0},
        0.7165313105737893,
        False,
        {"cpg.dangerous_calls": "1 dangerous-API call site(s) exceeds threshold (0.0)"},
    ),
    (
        "secure",
        {"dangerous_calls": 3.0},
        0.36787944117144233,
        False,
        {"cpg.dangerous_calls": "3 dangerous-API call site(s) exceeds threshold (0.0)"},
    ),
    (
        "secure",
        {"taint_flows": 0.0},
        1.0,
        True,
        {"cpg.taint_flows": "no source→sink taint paths (0 <= 0.0)"},
    ),
    (
        "secure",
        {"taint_flows": 1.0},
        0.7165313105737893,
        False,
        {"cpg.taint_flows": "1 taint flow path(s) exceeds threshold (0.0)"},
    ),
    (
        "secure",
        {"taint_flows": 3.0},
        0.36787944117144233,
        False,
        {"cpg.taint_flows": "3 taint flow path(s) exceeds threshold (0.0)"},
    ),
    (
        "secure",
        {"dangerous_calls": 2.0, "taint_flows": 1.0},
        0.513417119032592,
        False,
        {
            "cpg.dangerous_calls": "2 dangerous-API call site(s) exceeds threshold (0.0)",
            "cpg.taint_flows": "1 taint flow path(s) exceeds threshold (0.0)",
        },
    ),
]


@pytest.mark.parametrize(("pillar", "kwargs", "score", "achieved", "interp"), CASES)
def test_scorer_parity(pillar, kwargs, score, achieved, interp) -> None:
    decision = _SCORERS[pillar](**kwargs)
    assert decision.achieved is achieved
    assert decision.score == pytest.approx(score, abs=1e-12)
    assert dict(decision.interpretation) == interp
