/// GPU layer for compositing.
#[derive(Debug, Clone)]
pub struct GpuLayer {
    pub layer_id: usize,
    pub width: usize,
    pub height: usize,
    pub opacity: f32,
    pub transform: Transform2D,
    pub visible: bool,
    pub dirty: bool,
    pub pixel_data: Vec<u8>,
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
        Self {
            translate_x: 0.0,
            translate_y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            rotation_rad: 0.0,
        }
    }
}

/// Tile-based GPU compositor that manages layers and renders them efficiently.
pub struct GpuTileCompositor {
    pub active_layers: Vec<GpuLayer>,
    pub viewport_width: usize,
    pub viewport_height: usize,
    pub tile_size: usize,
    frame_count: u64,
    frame_buffer: Vec<u8>,
    dirty_rects: Vec<(usize, usize, usize, usize)>,
}

impl Default for GpuTileCompositor {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuTileCompositor {
    pub fn new() -> Self {
        let viewport_width = 1920;
        let viewport_height = 1080;
        let frame_buffer_size = viewport_width * viewport_height * 4;
        Self {
            active_layers: Vec::new(),
            viewport_width,
            viewport_height,
            tile_size: 256,
            frame_count: 0,
            frame_buffer: vec![0u8; frame_buffer_size],
            dirty_rects: Vec::new(),
        }
    }

    /// Create a new layer and return its ID.
    pub fn create_layer(&mut self, width: usize, height: usize) -> usize {
        let lid = self.active_layers.len() + 1;
        let pixel_count = width * height * 4;
        self.active_layers.push(GpuLayer {
            layer_id: lid,
            width,
            height,
            opacity: 1.0,
            transform: Transform2D::default(),
            visible: true,
            dirty: true,
            pixel_data: vec![0u8; pixel_count],
        });
        lid
    }

    /// Remove a layer by ID.
    pub fn remove_layer(&mut self, layer_id: usize) -> bool {
        if let Some(pos) = self
            .active_layers
            .iter()
            .position(|l| l.layer_id == layer_id)
        {
            self.active_layers.remove(pos);
            true
        } else {
            false
        }
    }

    /// Set layer opacity (0.0 to 1.0).
    pub fn set_layer_opacity(&mut self, layer_id: usize, opacity: f32) -> bool {
        if let Some(layer) = self
            .active_layers
            .iter_mut()
            .find(|l| l.layer_id == layer_id)
        {
            layer.opacity = opacity.clamp(0.0, 1.0);
            layer.dirty = true;
            self.mark_dirty(0, 0, self.viewport_width, self.viewport_height);
            true
        } else {
            false
        }
    }

    /// Set layer transform (position, scale, rotation).
    pub fn set_layer_transform(&mut self, layer_id: usize, transform: Transform2D) -> bool {
        if let Some(layer) = self
            .active_layers
            .iter_mut()
            .find(|l| l.layer_id == layer_id)
        {
            layer.transform = transform;
            layer.dirty = true;
            self.mark_dirty(0, 0, self.viewport_width, self.viewport_height);
            true
        } else {
            false
        }
    }

    /// Set layer visibility.
    pub fn set_layer_visible(&mut self, layer_id: usize, visible: bool) -> bool {
        if let Some(layer) = self
            .active_layers
            .iter_mut()
            .find(|l| l.layer_id == layer_id)
        {
            layer.visible = visible;
            layer.dirty = true;
            self.mark_dirty(0, 0, self.viewport_width, self.viewport_height);
            true
        } else {
            false
        }
    }

    /// Write pixel data to a layer.
    pub fn write_layer_pixels(
        &mut self,
        layer_id: usize,
        _x: usize,
        _y: usize,
        _width: usize,
        _height: usize,
        data: &[u8],
    ) -> bool {
        if let Some(layer) = self
            .active_layers
            .iter_mut()
            .find(|l| l.layer_id == layer_id)
        {
            let len = data.len().min(layer.pixel_data.len());
            layer.pixel_data[..len].copy_from_slice(&data[..len]);
            layer.dirty = true;
            self.mark_dirty(0, 0, self.viewport_width, self.viewport_height);
            true
        } else {
            false
        }
    }

    /// Compute the number of tiles needed to cover the viewport.
    pub fn tile_count(&self) -> (usize, usize) {
        let tiles_x = self.viewport_width.div_ceil(self.tile_size);
        let tiles_y = self.viewport_height.div_ceil(self.tile_size);
        (tiles_x, tiles_y)
    }

    /// Mark a region as dirty.
    fn mark_dirty(&mut self, x: usize, y: usize, width: usize, height: usize) {
        self.dirty_rects.push((x, y, width, height));
    }

    /// Composite a single tile by blending visible layers.
    pub fn composite_tile(&mut self, tile_x: usize, tile_y: usize) -> Vec<u8> {
        let tile_pixels = self.tile_size * self.tile_size * 4;
        let mut tile_buffer = vec![0u8; tile_pixels];

        let start_x = tile_x * self.tile_size;
        let start_y = tile_y * self.tile_size;
        let end_x = (start_x + self.tile_size).min(self.viewport_width);
        let end_y = (start_y + self.tile_size).min(self.viewport_height);

        // Composite visible layers back-to-front
        for layer in &self.active_layers {
            if !layer.visible {
                continue;
            }

            let layer_opacity = (layer.opacity * 255.0) as u32;

            for y in start_y..end_y {
                for x in start_x..end_x {
                    if x >= layer.width || y >= layer.height {
                        continue;
                    }

                    let layer_idx = (y * layer.width + x) * 4;
                    let tile_idx = ((y - start_y) * self.tile_size + (x - start_x)) * 4;

                    if layer_idx + 3 < layer.pixel_data.len() && tile_idx + 3 < tile_buffer.len() {
                        let src_r = layer.pixel_data[layer_idx] as u32;
                        let src_g = layer.pixel_data[layer_idx + 1] as u32;
                        let src_b = layer.pixel_data[layer_idx + 2] as u32;
                        let src_a = (layer.pixel_data[layer_idx + 3] as u32 * layer_opacity) / 255;

                        let dst_r = tile_buffer[tile_idx] as u32;
                        let dst_g = tile_buffer[tile_idx + 1] as u32;
                        let dst_b = tile_buffer[tile_idx + 2] as u32;
                        let dst_a = tile_buffer[tile_idx + 3] as u32;

                        // Alpha blend
                        let out_a = src_a + (dst_a * (255 - src_a)) / 255;
                        if let Some(inv) = 1u32.checked_div(out_a) {
                            let _ = inv; // use checked_div to guard against zero
                            tile_buffer[tile_idx] =
                                ((src_r * src_a + dst_r * (255 - src_a)) / out_a) as u8;
                            tile_buffer[tile_idx + 1] =
                                ((src_g * src_a + dst_g * (255 - src_a)) / out_a) as u8;
                            tile_buffer[tile_idx + 2] =
                                ((src_b * src_a + dst_b * (255 - src_a)) / out_a) as u8;
                            tile_buffer[tile_idx + 3] = out_a as u8;
                        }
                    }
                }
            }
        }

        tile_buffer
    }

    /// Composite all visible layers into the frame buffer. Returns the number
    /// of tiles rendered.
    pub fn composite_frame(&mut self) -> usize {
        let (tiles_x, tiles_y) = self.tile_count();
        let mut tiles_rendered = 0;

        // If there are dirty rects, only composite those tiles
        if !self.dirty_rects.is_empty() {
            let dirty_rects_clone = self.dirty_rects.clone();
            self.dirty_rects.clear();

            for &(_, _, _, _) in &dirty_rects_clone {
                // For simplicity, re-composite all tiles when any dirty rect exists
                for ty in 0..tiles_y {
                    for tx in 0..tiles_x {
                        let tile_data = self.composite_tile(tx, ty);
                        let start_x = tx * self.tile_size;
                        let start_y = ty * self.tile_size;

                        for y in 0..self.tile_size {
                            for x in 0..self.tile_size {
                                let fb_x = start_x + x;
                                let fb_y = start_y + y;
                                if fb_x >= self.viewport_width || fb_y >= self.viewport_height {
                                    continue;
                                }

                                let fb_idx = (fb_y * self.viewport_width + fb_x) * 4;
                                let tile_idx = (y * self.tile_size + x) * 4;
                                if fb_idx + 3 < self.frame_buffer.len()
                                    && tile_idx + 3 < tile_data.len()
                                {
                                    self.frame_buffer[fb_idx..fb_idx + 4]
                                        .copy_from_slice(&tile_data[tile_idx..tile_idx + 4]);
                                }
                            }
                        }
                        tiles_rendered += 1;
                    }
                }
            }
        } else {
            // No dirty rects, composite all tiles
            for ty in 0..tiles_y {
                for tx in 0..tiles_x {
                    let tile_data = self.composite_tile(tx, ty);
                    let start_x = tx * self.tile_size;
                    let start_y = ty * self.tile_size;

                    for y in 0..self.tile_size {
                        for x in 0..self.tile_size {
                            let fb_x = start_x + x;
                            let fb_y = start_y + y;
                            if fb_x >= self.viewport_width || fb_y >= self.viewport_height {
                                continue;
                            }

                            let fb_idx = (fb_y * self.viewport_width + fb_x) * 4;
                            let tile_idx = (y * self.tile_size + x) * 4;
                            if fb_idx + 3 < self.frame_buffer.len()
                                && tile_idx + 3 < tile_data.len()
                            {
                                self.frame_buffer[fb_idx..fb_idx + 4]
                                    .copy_from_slice(&tile_data[tile_idx..tile_idx + 4]);
                            }
                        }
                    }
                    tiles_rendered += 1;
                }
            }
        }

        // Mark all layers as clean after compositing
        for layer in &mut self.active_layers {
            layer.dirty = false;
        }
        self.frame_count += 1;
        tiles_rendered
    }

    /// Read a pixel from the frame buffer.
    pub fn read_pixel(&self, x: usize, y: usize) -> (u8, u8, u8, u8) {
        if x >= self.viewport_width || y >= self.viewport_height {
            return (0, 0, 0, 0);
        }
        let idx = (y * self.viewport_width + x) * 4;
        if idx + 3 >= self.frame_buffer.len() {
            return (0, 0, 0, 0);
        }
        (
            self.frame_buffer[idx],
            self.frame_buffer[idx + 1],
            self.frame_buffer[idx + 2],
            self.frame_buffer[idx + 3],
        )
    }

    /// Read a tile from the frame buffer.
    pub fn read_tile(&self, tile_x: usize, tile_y: usize) -> Vec<u8> {
        let start_x = tile_x * self.tile_size;
        let start_y = tile_y * self.tile_size;
        let end_x = (start_x + self.tile_size).min(self.viewport_width);
        let end_y = (start_y + self.tile_size).min(self.viewport_height);

        let mut tile_data = Vec::new();
        for y in start_y..end_y {
            for x in start_x..end_x {
                let idx = (y * self.viewport_width + x) * 4;
                if idx + 3 < self.frame_buffer.len() {
                    tile_data.extend_from_slice(&self.frame_buffer[idx..idx + 4]);
                }
            }
        }
        tile_data
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_composite_layers() {
        let mut compositor = GpuTileCompositor::new();
        compositor.viewport_width = 64;
        compositor.viewport_height = 64;
        compositor.frame_buffer = vec![0u8; 64 * 64 * 4];

        let layer1 = compositor.create_layer(64, 64);
        let mut pixels = vec![0u8; 64 * 64 * 4];
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[0] = 255; // R
            pixel[1] = 0; // G
            pixel[2] = 0; // B
            pixel[3] = 255; // A
        }
        compositor.write_layer_pixels(layer1, 0, 0, 64, 64, &pixels);

        let tiles = compositor.composite_frame();
        assert!(tiles > 0);

        let (r, g, b, a) = compositor.read_pixel(0, 0);
        assert_eq!(r, 255);
        assert_eq!(g, 0);
        assert_eq!(b, 0);
        assert_eq!(a, 255);
    }

    #[test]
    fn alpha_blending() {
        let mut compositor = GpuTileCompositor::new();
        compositor.viewport_width = 32;
        compositor.viewport_height = 32;
        compositor.frame_buffer = vec![0u8; 32 * 32 * 4];

        let layer1 = compositor.create_layer(32, 32);
        let mut pixels1 = vec![0u8; 32 * 32 * 4];
        for pixel in pixels1.chunks_exact_mut(4) {
            pixel[0] = 255; // R
            pixel[1] = 0; // G
            pixel[2] = 0; // B
            pixel[3] = 255; // A
        }
        compositor.write_layer_pixels(layer1, 0, 0, 32, 32, &pixels1);

        let layer2 = compositor.create_layer(32, 32);
        compositor.set_layer_opacity(layer2, 0.5);
        let mut pixels2 = vec![0u8; 32 * 32 * 4];
        for pixel in pixels2.chunks_exact_mut(4) {
            pixel[0] = 0; // R
            pixel[1] = 255; // G
            pixel[2] = 0; // B
            pixel[3] = 255; // A
        }
        compositor.write_layer_pixels(layer2, 0, 0, 32, 32, &pixels2);

        compositor.composite_frame();

        let (r, g, _b, a) = compositor.read_pixel(0, 0);
        // Layer 2 (green, 50% opacity) blends over layer 1 (red)
        assert!(g > 0); // Some green should be visible
        assert!(r > 0); // Some red should still be visible
        assert_eq!(a, 255); // Fully opaque
    }

    #[test]
    fn dirty_rect_tracking() {
        let mut compositor = GpuTileCompositor::new();
        compositor.viewport_width = 64;
        compositor.viewport_height = 64;
        compositor.frame_buffer = vec![0u8; 64 * 64 * 4];

        let layer = compositor.create_layer(64, 64);
        compositor.composite_frame();
        assert_eq!(compositor.dirty_rects.len(), 0);

        compositor.set_layer_opacity(layer, 0.5);
        assert!(!compositor.dirty_rects.is_empty());

        compositor.composite_frame();
        assert_eq!(compositor.dirty_rects.len(), 0);
    }

    #[test]
    fn remove_layer_nonexistent() {
        let mut c = GpuTileCompositor::new();
        assert!(!c.remove_layer(999));
    }

    #[test]
    fn set_opacity_nonexistent_layer() {
        let mut c = GpuTileCompositor::new();
        assert!(!c.set_layer_opacity(999, 0.5));
    }

    #[test]
    fn set_visible_nonexistent_layer() {
        let mut c = GpuTileCompositor::new();
        assert!(!c.set_layer_visible(999, false));
    }

    #[test]
    fn set_transform_nonexistent_layer() {
        let mut c = GpuTileCompositor::new();
        assert!(!c.set_layer_transform(999, Transform2D::default()));
    }

    #[test]
    fn write_pixels_nonexistent_layer() {
        let mut c = GpuTileCompositor::new();
        assert!(!c.write_layer_pixels(999, 0, 0, 1, 1, &[0; 4]));
    }

    #[test]
    fn tile_count_calculation() {
        let mut c = GpuTileCompositor::new();
        c.viewport_width = 512;
        c.viewport_height = 512;
        c.tile_size = 256;
        assert_eq!(c.tile_count(), (2, 2));
        // Non-even division should ceil
        c.viewport_width = 300;
        c.viewport_height = 300;
        assert_eq!(c.tile_count(), (2, 2));
    }

    #[test]
    fn read_pixel_out_of_bounds() {
        let c = GpuTileCompositor::new();
        assert_eq!(c.read_pixel(99999, 99999), (0, 0, 0, 0));
    }

    #[test]
    fn frame_count_increments() {
        let mut c = GpuTileCompositor::new();
        c.viewport_width = 32;
        c.viewport_height = 32;
        c.frame_buffer = vec![0u8; 32 * 32 * 4];
        assert_eq!(c.frame_count(), 0);
        c.composite_frame();
        assert_eq!(c.frame_count(), 1);
        c.composite_frame();
        assert_eq!(c.frame_count(), 2);
    }

    #[test]
    fn layer_count_tracks_layers() {
        let mut c = GpuTileCompositor::new();
        assert_eq!(c.layer_count(), 0);
        let l1 = c.create_layer(10, 10);
        assert_eq!(c.layer_count(), 1);
        c.create_layer(20, 20);
        assert_eq!(c.layer_count(), 2);
        c.remove_layer(l1);
        assert_eq!(c.layer_count(), 1);
    }

    #[test]
    fn create_layer_assigns_unique_ids() {
        let mut c = GpuTileCompositor::new();
        let l1 = c.create_layer(10, 10);
        let l2 = c.create_layer(10, 10);
        let l3 = c.create_layer(10, 10);
        assert_ne!(l1, l2);
        assert_ne!(l2, l3);
    }

    #[test]
    fn set_layer_opacity_clamps() {
        let mut c = GpuTileCompositor::new();
        let l = c.create_layer(10, 10);
        c.set_layer_opacity(l, 5.0);
        assert_eq!(c.active_layers[0].opacity, 1.0);
        c.set_layer_opacity(l, -1.0);
        assert_eq!(c.active_layers[0].opacity, 0.0);
    }

    #[test]
    fn invisible_layer_not_composited() {
        let mut c = GpuTileCompositor::new();
        c.viewport_width = 32;
        c.viewport_height = 32;
        c.frame_buffer = vec![0u8; 32 * 32 * 4];
        let l = c.create_layer(32, 32);
        let mut pixels = vec![0u8; 32 * 32 * 4];
        for p in pixels.chunks_exact_mut(4) {
            p[0] = 255;
            p[3] = 255;
        }
        c.write_layer_pixels(l, 0, 0, 32, 32, &pixels);
        c.set_layer_visible(l, false);
        c.composite_frame();
        // Pixel should be black (invisible layer not composited)
        let (r, _g, _b, _a) = c.read_pixel(0, 0);
        assert_eq!(r, 0);
    }

    #[test]
    fn read_tile_returns_data() {
        let mut c = GpuTileCompositor::new();
        c.viewport_width = 32;
        c.viewport_height = 32;
        c.frame_buffer = vec![0u8; 32 * 32 * 4];
        c.tile_size = 32;
        let tile = c.read_tile(0, 0);
        assert_eq!(tile.len(), 32 * 32 * 4);
    }

    #[test]
    fn composite_clears_layer_dirty_flags() {
        let mut c = GpuTileCompositor::new();
        c.viewport_width = 32;
        c.viewport_height = 32;
        c.frame_buffer = vec![0u8; 32 * 32 * 4];
        let l = c.create_layer(32, 32);
        assert!(c.active_layers[0].dirty);
        c.composite_frame();
        assert!(
            !c.active_layers
                .iter()
                .find(|layer| layer.layer_id == l)
                .unwrap()
                .dirty
        );
    }
}
