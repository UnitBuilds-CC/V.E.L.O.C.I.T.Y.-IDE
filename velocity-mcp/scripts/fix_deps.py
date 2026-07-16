path = 'Cargo.toml'
with open(path, 'r', encoding='utf-8') as f:
    text = f.read()

# Move pathdiff from [dev-dependencies] to [dependencies]
text = text.replace('pathdiff = "0.2.3"\n', '')
if 'pathdiff = "0.2.3"' not in text:
    text = text.replace('[dependencies]\n', '[dependencies]\npathdiff = "0.2.3"\n')

# tempfile dev is fine
with open(path, 'w', encoding='utf-8') as f:
    f.write(text)
print('Fixed deps')
