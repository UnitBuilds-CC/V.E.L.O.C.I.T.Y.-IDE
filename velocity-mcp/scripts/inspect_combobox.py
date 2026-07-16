import pathlib, re
p = pathlib.Path('C:/Users/visse/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-0.35.0/src/containers/combo_box.rs')
text = p.read_text(encoding='utf-8')
print(re.search(r'pub struct ComboBox[^;]*\{', text, re.S).group(0)[:600])
print('---')
for m in re.finditer(r'pub fn ([a-z_]+)\s*\(', text):
    print(m.group(1))
