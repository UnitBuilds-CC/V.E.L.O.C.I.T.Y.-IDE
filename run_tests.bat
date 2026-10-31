@echo off
cd /d "%~dp0"
cargo test -p velocity-ide --lib --jobs 2
echo EXIT_CODE=%ERRORLEVEL%
