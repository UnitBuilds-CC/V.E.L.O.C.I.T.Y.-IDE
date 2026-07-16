path = 'Cargo.toml'
with open(path, 'r', encoding='utf-8') as f:
    text = f.read()

text = text.replace('eframe = "0.35.0"', 'eframe = "0.26.2"')
text = text.replace('egui = "0.35.0"', 'egui = "0.26.2"')
text = text.replace('egui_dock = "0.20"', 'egui_dock = "0.11"')
text = text.replace('epaint = "0.35.0"', 'epaint = "0.26.2"')

with open(path, 'w', encoding='utf-8') as f:
    f.write(text)

print("Reverted Cargo.toml")
