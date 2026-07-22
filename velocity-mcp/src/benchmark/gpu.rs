use crate::compiler::driver::{
    VulkanBitNetLayer, VulkanDriver, VulkanNdaBitNetLayer, VulkanQwenLayer,
};

pub struct Qwen3BGpuLayerData {
    pub inputs_2304: Vec<f32>,
    pub out_2304_a: Vec<f32>,
    pub layer: VulkanQwenLayer,
}

impl Qwen3BGpuLayerData {
    pub fn new(driver: &VulkanDriver) -> Result<Self, Box<dyn std::error::Error>> {
        let inputs_2304 = vec![1.0f32; 2304];
        let out_2304_a = vec![0.0; 2304];

        let weight_q = vec![0x33u8; (2304 * 2304) / 2];
        let weight_o = vec![0x44u8; (2304 * 2304) / 2];
        let weight_k = vec![0x11u8; (2304 * 256) / 2];
        let weight_v = vec![0x22u8; (2304 * 256) / 2];
        let weight_gate = vec![0x77u8; (2304 * 11008) / 2];
        let weight_up = vec![0x88u8; (2304 * 11008) / 2];
        let weight_down = vec![0x99u8; (11008 * 2304) / 2];

        let layer = VulkanQwenLayer::new(
            driver,
            &weight_q,
            &weight_k,
            &weight_v,
            &weight_o,
            &weight_gate,
            &weight_up,
            &weight_down,
        )?;

        Ok(Self {
            inputs_2304,
            out_2304_a,
            layer,
        })
    }
}

pub struct BitNet3BGpuLayerData {
    pub inputs_3200: Vec<u32>,
    pub out_3200_down: Vec<f32>,
    pub layer: VulkanBitNetLayer,
}

impl BitNet3BGpuLayerData {
    pub fn new(driver: &VulkanDriver) -> Result<Self, Box<dyn std::error::Error>> {
        let inputs_3200 = vec![0x55555555u32; 3200 / 16];
        let out_3200_down = vec![0.0; 3200];

        let weight_q = vec![0x33333333u32; (3200 * 3200) / 16];
        let weight_k = vec![0x11111111u32; (3200 * 3200) / 16];
        let weight_v = vec![0x22222222u32; (3200 * 3200) / 16];
        let weight_o = vec![0x44444444u32; (3200 * 3200) / 16];
        let weight_gate = vec![0x77777777u32; (3200 * 8640) / 16];
        let weight_up = vec![0x88888888u32; (3200 * 8640) / 16];
        let weight_down = vec![0x99999999u32; (8640 * 3200) / 16];

        let to_bytes_u32 = |slice: &[u32]| -> &[u8] {
            unsafe { std::slice::from_raw_parts(slice.as_ptr() as *const u8, slice.len() * 4) }
        };

        let layer = VulkanBitNetLayer::new(
            driver,
            to_bytes_u32(&weight_q),
            to_bytes_u32(&weight_k),
            to_bytes_u32(&weight_v),
            to_bytes_u32(&weight_o),
            to_bytes_u32(&weight_gate),
            to_bytes_u32(&weight_up),
            to_bytes_u32(&weight_down),
        )?;

        Ok(Self {
            inputs_3200,
            out_3200_down,
            layer,
        })
    }
}

pub struct BitNet3BGpuNdaLayerData {
    pub inputs_active: Vec<u8>,
    pub inputs_pos: Vec<u8>,
    pub out_3200_down: Vec<f32>,
    pub layer: VulkanNdaBitNetLayer,
}

impl BitNet3BGpuNdaLayerData {
    pub fn new(driver: &VulkanDriver) -> Result<Self, Box<dyn std::error::Error>> {
        let inputs_3200 = vec![0x55555555u32; 3200 / 16];
        let out_3200_down = vec![0.0; 3200];

        let weight_q = vec![0x33333333u32; (3200 * 3200) / 16];
        let weight_k = vec![0x11111111u32; (3200 * 3200) / 16];
        let weight_v = vec![0x22222222u32; (3200 * 3200) / 16];
        let weight_o = vec![0x44444444u32; (3200 * 3200) / 16];
        let weight_gate = vec![0x77777777u32; (3200 * 8640) / 16];
        let weight_up = vec![0x88888888u32; (3200 * 8640) / 16];
        let weight_down = vec![0x99999999u32; (8640 * 3200) / 16];

        let to_bytes_u32 = |slice: &[u32]| -> &[u8] {
            unsafe { std::slice::from_raw_parts(slice.as_ptr() as *const u8, slice.len() * 4) }
        };

        let (in_act, in_pos) = crate::compiler::driver::pack_inputs_nda(&inputs_3200);
        let inputs_active = unsafe {
            std::slice::from_raw_parts(in_act.as_ptr() as *const u8, in_act.len() * 4).to_vec()
        };
        let inputs_pos = unsafe {
            std::slice::from_raw_parts(in_pos.as_ptr() as *const u8, in_pos.len() * 4).to_vec()
        };

        let layer = VulkanNdaBitNetLayer::new(
            driver,
            to_bytes_u32(&weight_q),
            to_bytes_u32(&weight_k),
            to_bytes_u32(&weight_v),
            to_bytes_u32(&weight_o),
            to_bytes_u32(&weight_gate),
            to_bytes_u32(&weight_up),
            to_bytes_u32(&weight_down),
        )?;

        Ok(Self {
            inputs_active,
            inputs_pos,
            out_3200_down,
            layer,
        })
    }
}

pub fn bench_qwen_3b_layer_gpu(
    data: &mut Qwen3BGpuLayerData,
) -> Result<f64, Box<dyn std::error::Error>> {
    let to_bytes_f32 = |slice: &[f32]| -> &[u8] {
        unsafe { std::slice::from_raw_parts(slice.as_ptr() as *const u8, slice.len() * 4) }
    };
    data.layer
        .run(to_bytes_f32(&data.inputs_2304), &mut data.out_2304_a)
}

pub fn bench_bitnet_3b_layer_gpu(
    data: &mut BitNet3BGpuLayerData,
) -> Result<f64, Box<dyn std::error::Error>> {
    let to_bytes_u32 = |slice: &[u32]| -> &[u8] {
        unsafe { std::slice::from_raw_parts(slice.as_ptr() as *const u8, slice.len() * 4) }
    };
    data.layer
        .run(to_bytes_u32(&data.inputs_3200), &mut data.out_3200_down)
}

pub fn bench_bitnet_3b_layer_gpu_nda(
    data: &mut BitNet3BGpuNdaLayerData,
) -> Result<f64, Box<dyn std::error::Error>> {
    data.layer.run(
        &data.inputs_active,
        &data.inputs_pos,
        &mut data.out_3200_down,
    )
}
