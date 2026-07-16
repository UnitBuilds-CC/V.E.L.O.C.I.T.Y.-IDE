import pathlib
p = pathlib.Path('C:/Users/visse/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-0.35.0/src')
for f in p.rglob('*.rs'):
    if f.name == 'layout_job.rs':
        print(f)
        print((f.parent / 'text_format.rs').exists())
