@echo off
cd /d C:\Users\visse\OneDrive\Documents\Velocity-IDE\Kimi-Code
cargo test -p velocity-ide --lib --jobs 2
echo EXIT_CODE=%ERRORLEVEL%
