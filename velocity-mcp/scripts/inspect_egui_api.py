import pathlib, re
base = pathlib.Path('C:/Users/visse/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-0.35.0/src')

# Panel usages summary
panel = (base / 'containers/panel.rs').read_text()
print('=== Panel impl methods ===')
for m in re.finditer(r'impl .*Panel[ \w]*\{', panel):
    print(m.group(0))
    start = panel.find('{', m.end())
    depth = 1
    i = start + 1
    while i < len(panel) and depth > 0:
        if panel[i] == '{': depth += 1
        elif panel[i] == '}': depth -= 1
        i += 1
    snippet = panel[start:i]
    for func in re.finditer(r'pub fn ([a-z_]+)', snippet):
        print(' ', func.group(1))

# Frame
frame_text = (base / 'containers/frame.rs').read_text()
print('\n=== Frame methods ===')
for m in re.finditer(r'pub fn ([a-z_]+)', frame_text):
    print(m.group(1))

# Visuals
vis = (base / 'style.rs').read_text()
print('\n=== Style methods / selection struct ===')
for m in re.finditer(r'selection:\s*pub\s+struct\s+(\w+)', vis):
    print('selection struct', m.group(1))
