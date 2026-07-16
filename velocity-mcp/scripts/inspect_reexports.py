import pathlib, re
p = pathlib.Path('C:/Users/visse/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-0.35.0/src/lib.rs')
text = p.read_text(encoding='utf-8')
print('TopBottomPanel' in text, 'SidePanel' in text, 'CentralPanel' in text)
print(re.findall(r'pub use (containers::[^;]+);', text)[:20])
print(re.findall(r'(pub use epaint::[^;]+);', text)[:20])
print(re.findall(r'(pub use self::[^;]+);', text)[:20])
