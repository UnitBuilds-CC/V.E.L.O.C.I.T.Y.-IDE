import re

registry_path = r"c:\Users\visse\OneDrive\Documentos\Kimi Code\velocity-workspace\velocity-mcp\src\registry.rs"

with open(registry_path, 'r', encoding='utf-8') as f:
    content = f.read()

fixed = content.replace(".map_err(|e| -> Box<dyn Error> { e.into() })", ".map_err(|e| Box::<dyn Error>::from(e))")

with open(registry_path, 'w', encoding='utf-8') as f:
    f.write(fixed)

print("Updated registry.rs map_err closures!")
