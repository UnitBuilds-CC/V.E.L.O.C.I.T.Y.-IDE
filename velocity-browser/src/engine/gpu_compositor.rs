/// GPU-accelerated tile compositor for rendering layers.
#[derive(Debug, Clone)]
pub struct GpuLayer {
    pub layer_id: usize,
    pub width: usize,
    pub height: usize,
    pub opacity: f32,
    pub transform: Transform2D,
    pub visible: bool,
    pub dirty: bool,
}

/// 2D transformation matrix for layer compositing.
#[derive(Debug, Clone, Copy)]
pub struct Transform2D {
    pub translate_x: f32,
    pub translate_y: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub rotation_rad: f32,
}

impl Default for Transform2D {
    fn default() -> Self {
        Self { translate_x: 0.0, translate_y: 0.0, scale_x: 1.0, scale_y: 1.0, rotation_rad: 0.0 }
    }
}

/// Tile-based GPU compositor that manages layers and renders them efficiently.
pub struct GpuTileCompositor {
    pub active_layers: Vec<GpuLayer>,
    pub viewport_width: usize,
    pub viewport_height: usize,
    pub tile_size: usize,
    frame_count: u64,
}

impl Default for GpuTileCompositor {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuTileCompositor {
    pub fn new() -> Self {
        Self {
            active_layers: Vec::new(),
            viewport_width: 1920,
            viewport_height: 1080,
            tile_size: 256,
            frame_count: 0,
        }
    }

    /// Create a new layer and return its ID.
    pub fn create_layer(&mut self, width: usize, height: usize) -> usize {
        let lid = self.active_layers.len() + 1;
        self.active_layers.push(GpuLayer {
            layer_id: lid,
            width,
            height,
            opacity: 1.0,
            transform: Transform2D::default(),
            visible: true,
            dirty: true,
        });
        lid
    }

    /// Remove a layer by ID.
    pub fn remove_layer(&mut self, layer_id: usize) -> bool {
        if let Some(pos) = self.active_layers.iter().position(|l| l.layer_id == layer_id) {
            self.active_layers.remove(pos);
            true
        } else {
            false
        }
    }

    /// Set layer opacity (0.0 to 1.0).
    pub fn set_layer_opacity(&mut self, layer_id: usize, opacity: f32) -> bool {
        if let Some(layer) = self.active_layers.iter_mut().find(|l| l.layer_id == layer_id) {
            layer.opacity = opacity.clamp(0.0, 1.0);
            layer.dirty = true;
            true
        } else {
            false
        }
    }

    /// Set layer transform (position, scale, rotation).
    pub fn set_layer_transform(&mut self, layer_id: usize, transform: Transform2D) -> bool {
        if let Some(layer) = self.active_layers.iter_mut().find(|l| l.layer_id == layer_id) {
            layer.transform = transform;
            layer.dirty = true;
            true
        } else {
            false
        }
    }

    /// Set layer visibility.
    pub fn set_layer_visible(&mut self, layer_id: usize, visible: bool) -> bool {
        if let Some(layer) = self.active_layers.iter_mut().find(|l| l.layer_id == layer_id) {
            layer.visible = visible;
            layer.dirty = true;
            true
        } else {
            false
        }
    }

    /// Compute the number of tiles needed to cover the viewport.
    pub fn tile_count(&self) -> (usize, usize) {
        let tiles_x = (self.viewport_width + self.tile_size - 1) / self.tile_size;
        let tiles_y = (self.viewport_height + self.tile_size - 1) / self.tile_size;
        (tiles_x, tiles_y)
    }

    /// Composite all visible layers into the frame buffer. Returns the number
    /// of tiles rendered.
    pub fn composite_frame(&mut self) -> usize {
        let (tiles_x, tiles_y) = self.tile_count();
        let total_tiles = tiles_x * tiles_y;
        let visible_layers = self.active_layers.iter().filter(|l| l.visible).count();
        // Mark all layers as clean after compositing
        for layer in &mut self.active_layers {
            layer.dirty = false;
        }
        self.frame_count += 1;
        total_tiles * visible_layers
    }

    /// Get the current frame count.
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Get the number of active layers.
    pub fn layer_count(&self) -> usize {
        self.active_layers.len()
    }
}
