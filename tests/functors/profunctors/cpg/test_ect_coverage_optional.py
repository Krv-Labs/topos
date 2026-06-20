from __future__ import annotations

from unittest.mock import patch

import pytest
from topos.functors.profunctors.cpg.topological_coverage import (
    ECTCoverageUnavailableError,
    calculate_topological_coverage,
    ect_coverage_available,
    require_ect_coverage,
)
from topos.graphs.cpg.object import CodePropertyGraph


def test_ect_coverage_available_when_deps_present():
    if not ect_coverage_available():
        pytest.skip("ect-coverage extra not installed in this environment")
    assert ect_coverage_available() is True
    require_ect_coverage()


def test_calculate_topological_coverage_raises_without_deps():
    with (
        patch(
            "topos.functors.profunctors.cpg.topological_coverage.ect_coverage_available",
            return_value=False,
        ),
        pytest.raises(ECTCoverageUnavailableError, match="ect-coverage"),
    ):
        calculate_topological_coverage(CodePropertyGraph(), CodePropertyGraph())
