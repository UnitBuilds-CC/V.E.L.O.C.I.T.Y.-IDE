import os, sys

engine_file = r"c:\Users\visse\OneDrive\Documentos\Kimi Code\velocity-workspace\velocity-mcp\src\editor\browser\engine.rs"
target_dir = r"c:\Users\visse\OneDrive\Documentos\Kimi Code\velocity-workspace\velocity-mcp\src\editor\browser\engine"

with open(engine_file, 'r', encoding='utf-8') as f:
    lines = f.readlines()

print(f"Total lines: {len(lines)}")
