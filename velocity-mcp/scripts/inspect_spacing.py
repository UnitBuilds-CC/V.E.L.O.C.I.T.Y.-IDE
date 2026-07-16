import pathlib, re
p = pathlib.Path('C:/Users/visse/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-0.35.0/src/style/spacing.rs')
text = p.read_text(encoding='utf-8')
print(re.search(r'pub struct Spacing\s*\{([^}]+)\}', text, re.S).group(0)[:2000])
