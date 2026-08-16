# egui 0.35 Native Immediate-Mode GUI with Dark HSL Theme

## Classification
- **Category**: UI Framework
- **Files**: velocity-mcp/src/editor/ (98 files)
- **Criticality**: High — all user interaction

## Summary

The Velocity IDE uses egui 0.35, a native immediate-mode GUI framework, for all user interface rendering. The theme uses a dark HSL color palette. All rendering is hardware-accelerated.

## Key Properties

- **Immediate mode**: UI is redrawn each frame from state
- **No web tech**: Pure native rendering, no HTML/CSS in the IDE itself
- **Dark HSL theme**: Consistent color palette via `editor/theme.rs`
- **VelocityApp struct**: Central state object owning all panel state

## Panel Architecture

- Code editor, chat panel, browser panel, orchestrator panel
- Smart sidebar, bottom panel, file tree, graph view
- Each panel is a separate module with its own render function
