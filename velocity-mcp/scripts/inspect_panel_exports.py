import pathlib, re
p = pathlib.Path('C:/Users/visse/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-0.35.0/src/containers/panel.rs')
text = p.read_text(encoding='utf-8')

# Find pub struct / pub enum
types = re.findall(r'pub (?:struct|enum|fn) ([A-Z][A-Za-z0-9_]*)', text)
print('Panel pub types:', set(types))

# Inspect lower-case variant names maybe TopBottomPanel is constructed via TopPanel/BottomPanel?
print(re.findall(r'pub fn ([a-z_]+)\(', text)[:30])

# show_all_panels?
# check mod.rs of containers
mp = pathlib.Path('C:/Users/visse/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-0.35.0/src/containers/mod.rs')
print('\ncontainers/mod.rs reexports:')
print(mp.read_text(encoding='utf-8')[:2000])
