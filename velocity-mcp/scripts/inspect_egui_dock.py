import pathlib, re

base = pathlib.Path('C:/Users/visse/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui_dock-0.20.0/src')

def show_struct(name):
    p = base / f'{name}.rs'
    if not p.exists():
        p = base / 'widgets.rs'
    if not p.exists():
        return
    text = p.read_text(encoding='utf-8')
    print(f'=== {name} ===')
    print(re.findall(r'pub fn ([a-z_]+)\s*\(', text)[:30])

for name in ['dock_state', 'tab_viewer', 'dock_area', 'style', 'leaf']:
    p = base / f'{name}.rs'
    if p.exists():
        text = p.read_text(encoding='utf-8')
        print(f'=== {name} ===')
        print(re.findall(r'pub fn ([a-z_]+)\s*\(', text)[:30])

# TabViewer trait
p = base / 'tab_viewer.rs'
if p.exists():
    text = p.read_text(encoding='utf-8')
    print('\n=== TabViewer trait ===')
    print(re.search(r'pub trait TabViewer[^{]*\{[^}]+\}', text, re.S).group(0)[:2000])
