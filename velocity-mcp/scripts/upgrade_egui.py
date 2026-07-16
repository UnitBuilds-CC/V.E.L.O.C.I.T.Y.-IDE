path = 'Cargo.toml'
with open(path, 'r', encoding='utf-8') as f:
    text = f.read()

replacements = {
    'eframe = "0.26.2"': 'eframe = "0.35.0"',
    'egui = "0.26.2"': 'egui = "0.35.0"',
    'egui_dock = "0.11"': 'egui_dock = "0.20"',
    'epaint = "0.26.2"': 'epaint = "0.35.0"',
}

for old, new in replacements.items():
    text = text.replace(old, new)

with open(path, 'w', encoding='utf-8') as f:
    f.write(text)

print("Updated Cargo.toml")
