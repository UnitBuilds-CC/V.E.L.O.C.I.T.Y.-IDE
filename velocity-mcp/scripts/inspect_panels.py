import pathlib, re
base = pathlib.Path('C:/Users/visse/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-0.35.0/src')

def show_methods(text, term):
    for m in re.finditer(rf'impl .*{term}.*\{{', text):
        start = text.find('{', m.end())
        depth = 1
        i = start + 1
        while i < len(text) and depth > 0:
            if text[i] == '{': depth += 1
            elif text[i] == '}': depth -= 1
            i += 1
        snippet = text[start:i]
        funcs = re.findall(r'pub fn ([a-z_]+)\s*\(', snippet)
        if funcs:
            print(term, funcs[:15])
        return

text = (base / 'containers/panel.rs').read_text()
for term in ['TopBottomPanel', 'SidePanel', 'CentralPanel']:
    show_methods(text, term)

frame_text = (base / 'containers/frame.rs').read_text()
print('Frame', re.findall(r'pub fn ([a-z_]+)\s*\(', frame_text)[:30])

# Margin/CornerRadius/Stroke/Color
try:
    margin = (base / 'margin.rs').read_text()
    print('Margin', re.findall(r'pub fn ([a-z_]+)\s*\(', margin)[:15])
except Exception as e:
    print('margin', e)

try:
    cr = (base / 'corner_radius.rs').read_text()
    print('CornerRadius', re.findall(r'pub fn ([a-z_]+)\s*\(', cr)[:10])
    print('CornerRadius consts', re.findall(r'pub const ([A-Z_]+)', cr)[:10])
except Exception as e:
    print('corner_radius', e)
