import pathlib, re

base = pathlib.Path('C:/Users/visse/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-0.35.0/src')

# Context methods
ctx_text = (base / 'context.rs').read_text(encoding='utf-8')
print('=== Context pub fn &self ===')
for m in re.finditer(r'pub fn ([a-z_]+)\s*\(&self', ctx_text):
    print(m.group(1))

# Ui style methods
ui_text = (base / 'ui.rs').read_text(encoding='utf-8')
print('\n=== Ui style methods ===')
for m in re.finditer(r'pub fn (style[a-z_]*)\s*\(', ui_text):
    print(m.group(1))

# Visuals fields
vis_text = (base / 'style.rs').read_text(encoding='utf-8')
print('\n=== Visuals fields ===')
print(re.search(r'pub struct Visuals\s*\{([^}]+)\}', vis_text, re.S).group(0)[:1200])
print('\n=== WidgetVisuals fields ===')
print(re.search(r'pub struct WidgetVisuals\s*\{([^}]+)\}', vis_text, re.S).group(0)[:800])

# Order
order_text = (base / 'layers.rs').read_text(encoding='utf-8')
print('\n=== Order variants ===')
print(re.search(r'pub enum Order\s*\{([^}]+)\}', order_text, re.S).group(0)[:500])

# Shadow
shadow_path = pathlib.Path('C:/Users/visse/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/epaint-0.35.0/src/shadow.rs')
if shadow_path.exists():
    shadow_text = shadow_path.read_text(encoding='utf-8')
    print('\n=== Shadow methods ===')
    print(re.findall(r'pub fn ([a-z_]+)\s*\(', shadow_text)[:20])
else:
    print('shadow path not found')
