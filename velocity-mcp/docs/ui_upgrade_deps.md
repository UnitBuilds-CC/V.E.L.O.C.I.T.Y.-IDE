# UI upgrade dependencies

Apply these changes to `Cargo.toml` before building:

```toml
[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
memmap2 = "0.9"
sha2 = "0.10"
once_cell = "1.18"
eframe = "0.26.2"
egui = "0.26.2"
egui_dock = "0.12"
epaint = "0.26.2"
ropey = "1.6"
crossbeam-channel = "0.5"
dotenvy = "0.15"
ureq = { version = "2.9", features = ["json"] }
ash = "0.37.3"
gpu-allocator = "0.25.0"
syntect = { version = "5.2", default-features = false, features = ["default-fancy"] }
log = "0.4"
```

Then run:

```bash
cargo update
cargo build
```
