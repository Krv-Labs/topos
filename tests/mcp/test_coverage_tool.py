
from topos.mcp.tools.coverage import topos_calculate_coverage
from topos.mcp.schemas import CalculateCoverageInput, ResponseFormat

params = CalculateCoverageInput(
    put_files=["src/topos/core/object.py"],
    test_files=["tests/topos/core/test_core.py"],
    language="python"
)

result = topos_calculate_coverage(params)
print(f"Mean Declaration Coverage: {result.mean_declaration_coverage}")
print(f"F2 Score: {result.f2_score}")
if result.error:
    print(f"Error: {result.error}")
else:
    print("Success!")
