import re

engine_file = r"c:\Users\visse\OneDrive\Documentos\Kimi Code\velocity-workspace\velocity-mcp\src\editor\browser\engine.rs"

with open(engine_file, 'r', encoding='utf-8') as f:
    lines = f.readlines()

def_pattern = re.compile(r'^(pub fn|fn|pub struct|struct|pub enum|enum|pub const|const)\s+([a-zA-Z0-9_]+)')

items = []
for idx, line in enumerate(lines):
    m = def_pattern.match(line)
    if m:
        items.append((idx + 1, m.group(1), m.group(2)))

print(f"Found {len(items)} top-level definitions:")
for line_num, kind, name in items:
    print(f"L{line_num}: {kind} {name}")
