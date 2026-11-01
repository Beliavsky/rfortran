@echo off
rem Every tests\test_*.py with the release compiler, then cargo test.
rem See full_tests.py for options, such as --no-cargo.
python "%~dp0full_tests.py" %*
