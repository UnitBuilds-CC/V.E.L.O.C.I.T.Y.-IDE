import pathlib, re
p = pathlib.Path('C:/Users/visse/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-0.35.0/src/widgets/text_edit/builder.rs')
text = p.read_text(encoding='utf-8')
print('=== TextEdit pub fn ===')
for m in re.finditer(r'pub fn ([a-z_]+)\s*\(', text):
    print(m.group(1))
print('===\n')
# LayoutJob and TextFormat
for file in ['text/layout_job.rs', 'style/text.rs']:
    try:
        p2 = pathlib.Path(f'C:/Users/visse/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-0.35.0/src/{file}')
        print(f'--- {file} ---')
        print(re.findall(r'pub fn ([a-z_]+)\s*\(', p2.read_text(encoding='utf-8'))[:20])
    except Exception as e:
        print(file, e)
