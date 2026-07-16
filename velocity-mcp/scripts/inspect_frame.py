import pathlib, re
p = pathlib.Path('C:/Users/visse/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/eframe-0.35.0/src/epi.rs')
text = p.read_text(encoding='utf-8')
for line in text.splitlines():
    if 'pub fn' in line and (' ui' in line or ' update' in line):
        print(line)
