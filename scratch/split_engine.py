import os, re

engine_path = r"c:\Users\visse\OneDrive\Documentos\Kimi Code\velocity-workspace\velocity-mcp\src\editor\browser\engine.rs"
out_dir = r"c:\Users\visse\OneDrive\Documentos\Kimi Code\velocity-workspace\velocity-mcp\src\editor\browser\engine"

os.makedirs(out_dir, exist_ok=True)

with open(engine_path, "r", encoding="utf-8") as f:
    lines = f.readlines()

print(f"Total lines in engine.rs: {len(lines)}")
