# Windows UI Automation FFI via COM Interfaces

## Classification
- **Category**: Platform Integration
- **Files**: velocity-mcp/src/wa/ (29 files)
- **Criticality**: High — desktop automation capability

## Summary

The WA module provides Windows desktop automation via direct COM FFI bindings to Windows UI Automation. It supports element selection, input simulation, screenshots, OCR, clipboard, window management, multi-monitor, virtual desktops, and process management.

## Architecture

- `uia_ffi.rs`: Raw COM interface bindings (IUIAutomation, IUIAutomationElement, etc.)
- `platform.rs`: Platform abstraction layer
- `selector.rs`: UI element targeting
- `advanced_input.rs`: Keyboard/mouse simulation
- `windows/`: Execution synthesis, payloads, scripts, reports

## Key Capabilities

- Element inspection and interaction
- Screenshot capture and OCR text extraction
- Clipboard read/write
- Window management and multi-monitor support
- Virtual desktop switching
- Process lifecycle management
- Native file dialogs
- Action recording and event triggers
