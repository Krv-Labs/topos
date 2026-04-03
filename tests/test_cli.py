import pytest
from click.testing import CliRunner
from topos.main import cli

def test_cli_help():
    runner = CliRunner()
    result = runner.invoke(cli, ['--help'])
    assert result.exit_code == 0
    assert 'Topos: Category-theoretic code quality evaluation' in result.output

def test_evaluate_command(tmp_path):
    runner = CliRunner()
    
    # Create a dummy python file
    p = tmp_path / "test_file.py"
    p.write_text("def my_func():\n    pass\n", encoding="utf-8")
    
    result = runner.invoke(cli, ['evaluate', str(p)])
    assert result.exit_code == 0
    assert 'test_file.py' in result.output
    assert 'Overall:' in result.output

def test_evaluate_command_json(tmp_path):
    runner = CliRunner()
    p = tmp_path / "test_file.py"
    p.write_text("def my_func():\n    pass\n", encoding="utf-8")
    
    result = runner.invoke(cli, ['evaluate', str(p), '--json'])
    assert result.exit_code == 0
    assert '"results":' in result.output

def test_compare_command(tmp_path):
    runner = CliRunner()
    p1 = tmp_path / "file1.py"
    p1.write_text("x = 1\n", encoding="utf-8")
    
    p2 = tmp_path / "file2.py"
    p2.write_text("y = 2\n", encoding="utf-8")
    
    result = runner.invoke(cli, ['compare', str(p1), str(p2), '--verbose'])
    assert result.exit_code == 0
    assert 'Edit Distance:' in result.output
    assert 'Operations:' in result.output

def test_inspect_command(tmp_path):
    runner = CliRunner()
    p = tmp_path / "inspect_file.py"
    p.write_text("def func(x):\n    return x + 1\n", encoding="utf-8")
    
    result = runner.invoke(cli, ['inspect', str(p)])
    assert result.exit_code == 0
    assert 'Classification' in result.output
    assert 'Metrics' in result.output
    assert 'func: 1' in result.output

def test_evaluate_no_paths():
    runner = CliRunner()
    result = runner.invoke(cli, ['evaluate'])
    # Because of the click argument decorator `paths` not having default empty, 
    # click might intercept it before our code. Wait, click nargs=-1 does not intercept.
    assert result.exit_code == 1

def test_evaluate_directory(tmp_path):
    runner = CliRunner()
    d = tmp_path / "src"
    d.mkdir()
    p = d / "test_file.py"
    p.write_text("def my_func():\n    pass\n", encoding="utf-8")
    
    result = runner.invoke(cli, ['evaluate', str(d), '-r'])
    assert result.exit_code == 0
    assert 'test_file.py' in result.output
