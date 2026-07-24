#[derive(Debug, Clone)]
pub struct GpuLayer {
    pub layer_id: usize,
    pub width: usize,
    pub height: usize,
    pub opacity: f32,
}

pub struct GpuTileCompositor {
    pub active_layers: Vec<GpuLayer>,
}

impl Default for GpuTileCompositor {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuTileCompositor {
    pub fn new() -> Self {
        Self { active_layers: Vec::new() }
    }

    pub fn create_layer(&mut self, width: usize, height: usize) -> usize {
        let lid = self.active_layers.len() + 1;
        self.active_layers.push(GpuLayer {
            layer_id: lid,
            width,
            height,
            opacity: 1.0,
        });
        lid
    }
}
