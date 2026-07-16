import pathlib, re
p = pathlib.Path('C:/Users/visse/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/eframe-0.35.0/src/epi.rs')
text = p.read_text()
match = re.search(r'trait App[^\{]*\{.*?^\}', text, re.S | re.M)
if match:
    print(match.group(0))
else:
    print("not found")
