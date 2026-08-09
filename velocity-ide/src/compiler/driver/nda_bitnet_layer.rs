use super::packing::*;
use super::vulkan_init::*;
use ash::vk;
use ash::Device;
use std::ffi::CString;
use std::time::Instant;

pub struct VulkanNdaBitNetLayer {
    pub device: Device,
    pub queue: vk::Queue,

    pub shader_nda: vk::ShaderModule,
    pub shader_act: vk::ShaderModule,

    pub desc_set_layout_nda: vk::DescriptorSetLayout,
    pub desc_set_layout_act: vk::DescriptorSetLayout,

    pub pipeline_layout_nda: vk::PipelineLayout,
    pub pipeline_layout_act: vk::PipelineLayout,

    pub pipeline_nda: vk::Pipeline,
    pub pipeline_act: vk::Pipeline,

    pub inputs_3200_active_buffer: vk::Buffer,
    pub inputs_3200_active_memory: vk::DeviceMemory,
    pub inputs_3200_active_ptr: *mut std::ffi::c_void,

    pub inputs_3200_pos_buffer: vk::Buffer,
    pub inputs_3200_pos_memory: vk::DeviceMemory,
    pub inputs_3200_pos_ptr: *mut std::ffi::c_void,

    pub out_3200_q_buffer: vk::Buffer,
    pub out_3200_q_memory: vk::DeviceMemory,

    pub out_3200_k_buffer: vk::Buffer,
    pub out_3200_k_memory: vk::DeviceMemory,

    pub out_3200_v_buffer: vk::Buffer,
    pub out_3200_v_memory: vk::DeviceMemory,

    pub out_3200_o_buffer: vk::Buffer,
    pub out_3200_o_memory: vk::DeviceMemory,

    pub out_8640_gate_buffer: vk::Buffer,
    pub out_8640_gate_memory: vk::DeviceMemory,

    pub out_8640_up_buffer: vk::Buffer,
    pub out_8640_up_memory: vk::DeviceMemory,

    pub inputs_8640_active_buffer: vk::Buffer,
    pub inputs_8640_active_memory: vk::DeviceMemory,

    pub inputs_8640_pos_buffer: vk::Buffer,
    pub inputs_8640_pos_memory: vk::DeviceMemory,

    pub out_3200_down_buffer: vk::Buffer,
    pub out_3200_down_memory: vk::DeviceMemory,
    pub out_3200_down_ptr: *mut std::ffi::c_void,

    pub weight_q_active_buffer: vk::Buffer,
    pub weight_q_active_memory: vk::DeviceMemory,
    pub weight_q_pos_buffer: vk::Buffer,
    pub weight_q_pos_memory: vk::DeviceMemory,

    pub weight_k_active_buffer: vk::Buffer,
    pub weight_k_active_memory: vk::DeviceMemory,
    pub weight_k_pos_buffer: vk::Buffer,
    pub weight_k_pos_memory: vk::DeviceMemory,

    pub weight_v_active_buffer: vk::Buffer,
    pub weight_v_active_memory: vk::DeviceMemory,
    pub weight_v_pos_buffer: vk::Buffer,
    pub weight_v_pos_memory: vk::DeviceMemory,

    pub weight_o_active_buffer: vk::Buffer,
    pub weight_o_active_memory: vk::DeviceMemory,
    pub weight_o_pos_buffer: vk::Buffer,
    pub weight_o_pos_memory: vk::DeviceMemory,

    pub weight_gate_active_buffer: vk::Buffer,
    pub weight_gate_active_memory: vk::DeviceMemory,
    pub weight_gate_pos_buffer: vk::Buffer,
    pub weight_gate_pos_memory: vk::DeviceMemory,

    pub weight_up_active_buffer: vk::Buffer,
    pub weight_up_active_memory: vk::DeviceMemory,
    pub weight_up_pos_buffer: vk::Buffer,
    pub weight_up_pos_memory: vk::DeviceMemory,

    pub weight_down_active_buffer: vk::Buffer,
    pub weight_down_active_memory: vk::DeviceMemory,
    pub weight_down_pos_buffer: vk::Buffer,
    pub weight_down_pos_memory: vk::DeviceMemory,

    pub desc_pool: vk::DescriptorPool,
    pub desc_sets_nda: Vec<vk::DescriptorSet>,
    pub desc_set_act: vk::DescriptorSet,

    pub command_pool: vk::CommandPool,
    pub command_buffer: vk::CommandBuffer,
    pub fence: vk::Fence,
}

impl VulkanNdaBitNetLayer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        driver: &VulkanDriver,
        weight_q: &[u8],
        weight_k: &[u8],
        weight_v: &[u8],
        weight_o: &[u8],
        weight_gate: &[u8],
        weight_up: &[u8],
        weight_down: &[u8],
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let device = driver.device.clone();
        let physical_device = driver.physical_device;
        let instance = driver.instance.clone();
        let queue = driver.compute_queue;

        let shader_info_nda =
            vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::NDA_SPV);
        let shader_nda = unsafe { device.create_shader_module(&shader_info_nda, None)? };

        let shader_info_act =
            vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::ACT_NDA_SPV);
        let shader_act = unsafe { device.create_shader_module(&shader_info_act, None)? };

        let bindings_nda = [
            vk::DescriptorSetLayoutBinding::builder()
                .binding(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .build(),
            vk::DescriptorSetLayoutBinding::builder()
                .binding(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .build(),
            vk::DescriptorSetLayoutBinding::builder()
                .binding(2)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .build(),
            vk::DescriptorSetLayoutBinding::builder()
                .binding(3)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .build(),
            vk::DescriptorSetLayoutBinding::builder()
                .binding(4)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .build(),
        ];
        let layout_info_nda = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings_nda);
        let desc_set_layout_nda =
            unsafe { device.create_descriptor_set_layout(&layout_info_nda, None)? };

        let bindings_act = [
            vk::DescriptorSetLayoutBinding::builder()
                .binding(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .build(),
            vk::DescriptorSetLayoutBinding::builder()
                .binding(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .build(),
            vk::DescriptorSetLayoutBinding::builder()
                .binding(2)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .build(),
            vk::DescriptorSetLayoutBinding::builder()
                .binding(3)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .build(),
        ];
        let layout_info_act = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings_act);
        let desc_set_layout_act =
            unsafe { device.create_descriptor_set_layout(&layout_info_act, None)? };

        let push_constant_ranges = [vk::PushConstantRange::builder()
            .stage_flags(vk::ShaderStageFlags::COMPUTE)
            .offset(0)
            .size(8)
            .build()];

        let layouts_nda = [desc_set_layout_nda];
        let pipeline_layout_info_nda = vk::PipelineLayoutCreateInfo::builder()
            .set_layouts(&layouts_nda)
            .push_constant_ranges(&push_constant_ranges);
        let pipeline_layout_nda =
            unsafe { device.create_pipeline_layout(&pipeline_layout_info_nda, None)? };

        let layouts_act = [desc_set_layout_act];
        let pipeline_layout_info_act = vk::PipelineLayoutCreateInfo::builder()
            .set_layouts(&layouts_act)
            .push_constant_ranges(&push_constant_ranges);
        let pipeline_layout_act =
            unsafe { device.create_pipeline_layout(&pipeline_layout_info_act, None)? };

        let main_entry = CString::new("main")?;

        let stage_info_nda = vk::PipelineShaderStageCreateInfo::builder()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(shader_nda)
            .name(&main_entry);
        let pipeline_create_info_nda = vk::ComputePipelineCreateInfo::builder()
            .stage(stage_info_nda.build())
            .layout(pipeline_layout_nda);

        let stage_info_act = vk::PipelineShaderStageCreateInfo::builder()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(shader_act)
            .name(&main_entry);
        let pipeline_create_info_act = vk::ComputePipelineCreateInfo::builder()
            .stage(stage_info_act.build())
            .layout(pipeline_layout_act);

        let pipelines_nda = unsafe {
            device
                .create_compute_pipelines(
                    vk::PipelineCache::null(),
                    &[pipeline_create_info_nda.build()],
                    None,
                )
                .map_err(|(_, e)| e)?
        };
        let pipeline_nda = pipelines_nda[0];

        let pipelines_act = unsafe {
            device
                .create_compute_pipelines(
                    vk::PipelineCache::null(),
                    &[pipeline_create_info_act.build()],
                    None,
                )
                .map_err(|(_, e)| e)?
        };
        let pipeline_act = pipelines_act[0];

        let in_3200_bytes = (3200 / 32) * 4;
        let (inputs_3200_active_buffer, inputs_3200_active_memory, inputs_3200_active_ptr) =
            create_coherent_buffer(
                &device,
                &instance,
                physical_device,
                in_3200_bytes as vk::DeviceSize,
                vk::BufferUsageFlags::STORAGE_BUFFER,
            )?;
        let (inputs_3200_pos_buffer, inputs_3200_pos_memory, inputs_3200_pos_ptr) =
            create_coherent_buffer(
                &device,
                &instance,
                physical_device,
                in_3200_bytes as vk::DeviceSize,
                vk::BufferUsageFlags::STORAGE_BUFFER,
            )?;

        let out_3200_bytes = 3200 * 4;
        let (out_3200_q_buffer, out_3200_q_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            out_3200_bytes as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (out_3200_k_buffer, out_3200_k_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            out_3200_bytes as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (out_3200_v_buffer, out_3200_v_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            out_3200_bytes as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (out_3200_o_buffer, out_3200_o_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            out_3200_bytes as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;

        let out_8640_bytes = 8640 * 4;
        let (out_8640_gate_buffer, out_8640_gate_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            out_8640_bytes as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (out_8640_up_buffer, out_8640_up_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            out_8640_bytes as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;

        let in_8640_bytes = (8640 / 32) * 4;
        let (inputs_8640_active_buffer, inputs_8640_active_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            in_8640_bytes as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (inputs_8640_pos_buffer, inputs_8640_pos_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            in_8640_bytes as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;

        let (out_3200_down_buffer, out_3200_down_memory, out_3200_down_ptr) =
            create_coherent_buffer(
                &device,
                &instance,
                physical_device,
                out_3200_bytes as vk::DeviceSize,
                vk::BufferUsageFlags::STORAGE_BUFFER,
            )?;

        let pack = |w: &[u8], k, n| pack_weights_nda(w, k, n);

        let (wq_a, wq_p) = pack(weight_q, 3200, 3200);
        let (weight_q_active_buffer, weight_q_active_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wq_a.len() as vk::DeviceSize,
            &wq_a,
        )?;
        let (weight_q_pos_buffer, weight_q_pos_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wq_p.len() as vk::DeviceSize,
            &wq_p,
        )?;

        let (wk_a, wk_p) = pack(weight_k, 3200, 3200);
        let (weight_k_active_buffer, weight_k_active_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wk_a.len() as vk::DeviceSize,
            &wk_a,
        )?;
        let (weight_k_pos_buffer, weight_k_pos_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wk_p.len() as vk::DeviceSize,
            &wk_p,
        )?;

        let (wv_a, wv_p) = pack(weight_v, 3200, 3200);
        let (weight_v_active_buffer, weight_v_active_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wv_a.len() as vk::DeviceSize,
            &wv_a,
        )?;
        let (weight_v_pos_buffer, weight_v_pos_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wv_p.len() as vk::DeviceSize,
            &wv_p,
        )?;

        let (wo_a, wo_p) = pack(weight_o, 3200, 3200);
        let (weight_o_active_buffer, weight_o_active_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wo_a.len() as vk::DeviceSize,
            &wo_a,
        )?;
        let (weight_o_pos_buffer, weight_o_pos_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wo_p.len() as vk::DeviceSize,
            &wo_p,
        )?;

        let (wgate_a, wgate_p) = pack(weight_gate, 3200, 8640);
        let (weight_gate_active_buffer, weight_gate_active_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wgate_a.len() as vk::DeviceSize,
            &wgate_a,
        )?;
        let (weight_gate_pos_buffer, weight_gate_pos_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wgate_p.len() as vk::DeviceSize,
            &wgate_p,
        )?;

        let (wup_a, wup_p) = pack(weight_up, 3200, 8640);
        let (weight_up_active_buffer, weight_up_active_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wup_a.len() as vk::DeviceSize,
            &wup_a,
        )?;
        let (weight_up_pos_buffer, weight_up_pos_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wup_p.len() as vk::DeviceSize,
            &wup_p,
        )?;

        let (wdown_a, wdown_p) = pack(weight_down, 8640, 3200);
        let (weight_down_active_buffer, weight_down_active_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wdown_a.len() as vk::DeviceSize,
            &wdown_a,
        )?;
        let (weight_down_pos_buffer, weight_down_pos_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wdown_p.len() as vk::DeviceSize,
            &wdown_p,
        )?;

        let pool_sizes = [vk::DescriptorPoolSize::builder()
            .ty(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(40)
            .build()];
        let pool_info = vk::DescriptorPoolCreateInfo::builder()
            .max_sets(8)
            .pool_sizes(&pool_sizes);
        let desc_pool = unsafe { device.create_descriptor_pool(&pool_info, None)? };

        let layouts_nda = vec![desc_set_layout_nda; 7];
        let desc_sets_nda = unsafe {
            device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::builder()
                    .descriptor_pool(desc_pool)
                    .set_layouts(&layouts_nda),
            )?
        };

        let desc_set_act = unsafe {
            device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::builder()
                    .descriptor_pool(desc_pool)
                    .set_layouts(&[desc_set_layout_act]),
            )?[0]
        };

        let set_configs_nda = [
            (
                inputs_3200_active_buffer,
                inputs_3200_pos_buffer,
                weight_q_active_buffer,
                weight_q_pos_buffer,
                out_3200_q_buffer,
            ),
            (
                inputs_3200_active_buffer,
                inputs_3200_pos_buffer,
                weight_k_active_buffer,
                weight_k_pos_buffer,
                out_3200_k_buffer,
            ),
            (
                inputs_3200_active_buffer,
                inputs_3200_pos_buffer,
                weight_v_active_buffer,
                weight_v_pos_buffer,
                out_3200_v_buffer,
            ),
            (
                inputs_3200_active_buffer,
                inputs_3200_pos_buffer,
                weight_o_active_buffer,
                weight_o_pos_buffer,
                out_3200_o_buffer,
            ),
            (
                inputs_3200_active_buffer,
                inputs_3200_pos_buffer,
                weight_gate_active_buffer,
                weight_gate_pos_buffer,
                out_8640_gate_buffer,
            ),
            (
                inputs_3200_active_buffer,
                inputs_3200_pos_buffer,
                weight_up_active_buffer,
                weight_up_pos_buffer,
                out_8640_up_buffer,
            ),
            (
                inputs_8640_active_buffer,
                inputs_8640_pos_buffer,
                weight_down_active_buffer,
                weight_down_pos_buffer,
                out_3200_down_buffer,
            ),
        ];

        for (i, (in_a, in_p, w_a, w_p, out_b)) in set_configs_nda.iter().enumerate() {
            let buffer_infos = [
                vk::DescriptorBufferInfo::builder()
                    .buffer(*in_a)
                    .offset(0)
                    .range(vk::WHOLE_SIZE)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(*in_p)
                    .offset(0)
                    .range(vk::WHOLE_SIZE)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(*w_a)
                    .offset(0)
                    .range(vk::WHOLE_SIZE)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(*w_p)
                    .offset(0)
                    .range(vk::WHOLE_SIZE)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(*out_b)
                    .offset(0)
                    .range(vk::WHOLE_SIZE)
                    .build(),
            ];
            let writes = [
                vk::WriteDescriptorSet::builder()
                    .dst_set(desc_sets_nda[i])
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&buffer_infos[0..1])
                    .build(),
                vk::WriteDescriptorSet::builder()
                    .dst_set(desc_sets_nda[i])
                    .dst_binding(1)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&buffer_infos[1..2])
                    .build(),
                vk::WriteDescriptorSet::builder()
                    .dst_set(desc_sets_nda[i])
                    .dst_binding(2)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&buffer_infos[2..3])
                    .build(),
                vk::WriteDescriptorSet::builder()
                    .dst_set(desc_sets_nda[i])
                    .dst_binding(3)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&buffer_infos[3..4])
                    .build(),
                vk::WriteDescriptorSet::builder()
                    .dst_set(desc_sets_nda[i])
                    .dst_binding(4)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&buffer_infos[4..5])
                    .build(),
            ];
            unsafe { device.update_descriptor_sets(&writes, &[]) };
        }

        let buffer_infos_act = [
            vk::DescriptorBufferInfo::builder()
                .buffer(out_8640_gate_buffer)
                .offset(0)
                .range(vk::WHOLE_SIZE)
                .build(),
            vk::DescriptorBufferInfo::builder()
                .buffer(out_8640_up_buffer)
                .offset(0)
                .range(vk::WHOLE_SIZE)
                .build(),
            vk::DescriptorBufferInfo::builder()
                .buffer(inputs_8640_active_buffer)
                .offset(0)
                .range(vk::WHOLE_SIZE)
                .build(),
            vk::DescriptorBufferInfo::builder()
                .buffer(inputs_8640_pos_buffer)
                .offset(0)
                .range(vk::WHOLE_SIZE)
                .build(),
        ];
        let writes_act = [
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set_act)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos_act[0..1])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set_act)
                .dst_binding(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos_act[1..2])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set_act)
                .dst_binding(2)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos_act[2..3])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set_act)
                .dst_binding(3)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos_act[3..4])
                .build(),
        ];
        unsafe { device.update_descriptor_sets(&writes_act, &[]) };

        let pool_info = vk::CommandPoolCreateInfo::builder()
            .queue_family_index(driver.queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = unsafe { device.create_command_pool(&pool_info, None)? };

        let alloc_info = vk::CommandBufferAllocateInfo::builder()
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        let command_buffers = unsafe { device.allocate_command_buffers(&alloc_info)? };
        let command_buffer = command_buffers[0];

        let begin_info = vk::CommandBufferBeginInfo::builder()
            .flags(vk::CommandBufferUsageFlags::SIMULTANEOUS_USE);
        unsafe {
            device.begin_command_buffer(command_buffer, &begin_info)?;

            let dispatch_nda = |cmd: vk::CommandBuffer, set: vk::DescriptorSet, k: u32, n: u32| {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, pipeline_nda);
                device.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::COMPUTE,
                    pipeline_layout_nda,
                    0,
                    &[set],
                    &[],
                );
                let params = [k, n];
                let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
                device.cmd_push_constants(
                    cmd,
                    pipeline_layout_nda,
                    vk::ShaderStageFlags::COMPUTE,
                    0,
                    params_bytes,
                );
                let workgroups = n.div_ceil(256u32);
                device.cmd_dispatch(cmd, workgroups, 1, 1);
            };

            let dispatch_act = |cmd: vk::CommandBuffer, set: vk::DescriptorSet, n: u32| {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, pipeline_act);
                device.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::COMPUTE,
                    pipeline_layout_act,
                    0,
                    &[set],
                    &[],
                );
                let params = [n, 0u32];
                let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
                device.cmd_push_constants(
                    cmd,
                    pipeline_layout_act,
                    vk::ShaderStageFlags::COMPUTE,
                    0,
                    params_bytes,
                );
                let workgroups = n.div_ceil(256u32);
                device.cmd_dispatch(cmd, workgroups, 1, 1);
            };

            dispatch_nda(command_buffer, desc_sets_nda[0], 3200, 3200);
            dispatch_nda(command_buffer, desc_sets_nda[1], 3200, 3200);
            dispatch_nda(command_buffer, desc_sets_nda[2], 3200, 3200);
            dispatch_nda(command_buffer, desc_sets_nda[3], 3200, 3200);
            dispatch_nda(command_buffer, desc_sets_nda[4], 3200, 8640);
            dispatch_nda(command_buffer, desc_sets_nda[5], 3200, 8640);
            dispatch_act(command_buffer, desc_set_act, 8640);
            dispatch_nda(command_buffer, desc_sets_nda[6], 8640, 3200);

            device.end_command_buffer(command_buffer)?;
        }

        let fence_info = vk::FenceCreateInfo::builder();
        let fence = unsafe { device.create_fence(&fence_info, None)? };

        Ok(Self {
            device,
            queue,
            shader_nda,
            shader_act,
            desc_set_layout_nda,
            desc_set_layout_act,
            pipeline_layout_nda,
            pipeline_layout_act,
            pipeline_nda,
            pipeline_act,
            inputs_3200_active_buffer,
            inputs_3200_active_memory,
            inputs_3200_active_ptr,
            inputs_3200_pos_buffer,
            inputs_3200_pos_memory,
            inputs_3200_pos_ptr,
            out_3200_q_buffer,
            out_3200_q_memory,
            out_3200_k_buffer,
            out_3200_k_memory,
            out_3200_v_buffer,
            out_3200_v_memory,
            out_3200_o_buffer,
            out_3200_o_memory,
            out_8640_gate_buffer,
            out_8640_gate_memory,
            out_8640_up_buffer,
            out_8640_up_memory,
            inputs_8640_active_buffer,
            inputs_8640_active_memory,
            inputs_8640_pos_buffer,
            inputs_8640_pos_memory,
            out_3200_down_buffer,
            out_3200_down_memory,
            out_3200_down_ptr,
            weight_q_active_buffer,
            weight_q_active_memory,
            weight_q_pos_buffer,
            weight_q_pos_memory,
            weight_k_active_buffer,
            weight_k_active_memory,
            weight_k_pos_buffer,
            weight_k_pos_memory,
            weight_v_active_buffer,
            weight_v_active_memory,
            weight_v_pos_buffer,
            weight_v_pos_memory,
            weight_o_active_buffer,
            weight_o_active_memory,
            weight_o_pos_buffer,
            weight_o_pos_memory,
            weight_gate_active_buffer,
            weight_gate_active_memory,
            weight_gate_pos_buffer,
            weight_gate_pos_memory,
            weight_up_active_buffer,
            weight_up_active_memory,
            weight_up_pos_buffer,
            weight_up_pos_memory,
            weight_down_active_buffer,
            weight_down_active_memory,
            weight_down_pos_buffer,
            weight_down_pos_memory,
            desc_pool,
            desc_sets_nda,
            desc_set_act,
            command_pool,
            command_buffer,
            fence,
        })
    }

    pub fn run(
        &self,
        inputs_active_bytes: &[u8],
        inputs_pos_bytes: &[u8],
        output_floats: &mut [f32],
    ) -> Result<f64, Box<dyn std::error::Error>> {
        unsafe {
            std::ptr::copy_nonoverlapping(
                inputs_active_bytes.as_ptr(),
                self.inputs_3200_active_ptr as *mut u8,
                inputs_active_bytes.len(),
            );
            std::ptr::copy_nonoverlapping(
                inputs_pos_bytes.as_ptr(),
                self.inputs_3200_pos_ptr as *mut u8,
                inputs_pos_bytes.len(),
            );
        }

        let start = Instant::now();
        let command_buffers = [self.command_buffer];
        let submit_info = vk::SubmitInfo::builder().command_buffers(&command_buffers);
        unsafe {
            self.device.reset_fences(&[self.fence])?;
            self.device
                .queue_submit(self.queue, &[submit_info.build()], self.fence)?;
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        let duration_us = start.elapsed().as_micros() as f64;

        unsafe {
            std::ptr::copy_nonoverlapping(
                self.out_3200_down_ptr as *const f32,
                output_floats.as_mut_ptr(),
                output_floats.len(),
            );
        }

        Ok(duration_us)
    }
}

impl Drop for VulkanNdaBitNetLayer {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_descriptor_pool(self.desc_pool, None);

            self.device.unmap_memory(self.inputs_3200_active_memory);
            self.device
                .free_memory(self.inputs_3200_active_memory, None);
            self.device
                .destroy_buffer(self.inputs_3200_active_buffer, None);

            self.device.unmap_memory(self.inputs_3200_pos_memory);
            self.device.free_memory(self.inputs_3200_pos_memory, None);
            self.device
                .destroy_buffer(self.inputs_3200_pos_buffer, None);

            self.device.unmap_memory(self.out_3200_down_memory);
            self.device.free_memory(self.out_3200_down_memory, None);
            self.device.destroy_buffer(self.out_3200_down_buffer, None);

            self.device.free_memory(self.out_3200_q_memory, None);
            self.device.destroy_buffer(self.out_3200_q_buffer, None);

            self.device.free_memory(self.out_3200_k_memory, None);
            self.device.destroy_buffer(self.out_3200_k_buffer, None);

            self.device.free_memory(self.out_3200_v_memory, None);
            self.device.destroy_buffer(self.out_3200_v_buffer, None);

            self.device.free_memory(self.out_3200_o_memory, None);
            self.device.destroy_buffer(self.out_3200_o_buffer, None);

            self.device.free_memory(self.out_8640_gate_memory, None);
            self.device.destroy_buffer(self.out_8640_gate_buffer, None);

            self.device.free_memory(self.out_8640_up_memory, None);
            self.device.destroy_buffer(self.out_8640_up_buffer, None);

            self.device
                .free_memory(self.inputs_8640_active_memory, None);
            self.device
                .destroy_buffer(self.inputs_8640_active_buffer, None);

            self.device.free_memory(self.inputs_8640_pos_memory, None);
            self.device
                .destroy_buffer(self.inputs_8640_pos_buffer, None);

            self.device.free_memory(self.weight_q_active_memory, None);
            self.device
                .destroy_buffer(self.weight_q_active_buffer, None);
            self.device.free_memory(self.weight_q_pos_memory, None);
            self.device.destroy_buffer(self.weight_q_pos_buffer, None);

            self.device.free_memory(self.weight_k_active_memory, None);
            self.device
                .destroy_buffer(self.weight_k_active_buffer, None);
            self.device.free_memory(self.weight_k_pos_memory, None);
            self.device.destroy_buffer(self.weight_k_pos_buffer, None);

            self.device.free_memory(self.weight_v_active_memory, None);
            self.device
                .destroy_buffer(self.weight_v_active_buffer, None);
            self.device.free_memory(self.weight_v_pos_memory, None);
            self.device.destroy_buffer(self.weight_v_pos_buffer, None);

            self.device.free_memory(self.weight_o_active_memory, None);
            self.device
                .destroy_buffer(self.weight_o_active_buffer, None);
            self.device.free_memory(self.weight_o_pos_memory, None);
            self.device.destroy_buffer(self.weight_o_pos_buffer, None);

            self.device
                .free_memory(self.weight_gate_active_memory, None);
            self.device
                .destroy_buffer(self.weight_gate_active_buffer, None);
            self.device.free_memory(self.weight_gate_pos_memory, None);
            self.device
                .destroy_buffer(self.weight_gate_pos_buffer, None);

            self.device.free_memory(self.weight_up_active_memory, None);
            self.device
                .destroy_buffer(self.weight_up_active_buffer, None);
            self.device.free_memory(self.weight_up_pos_memory, None);
            self.device.destroy_buffer(self.weight_up_pos_buffer, None);

            self.device
                .free_memory(self.weight_down_active_memory, None);
            self.device
                .destroy_buffer(self.weight_down_active_buffer, None);
            self.device.free_memory(self.weight_down_pos_memory, None);
            self.device
                .destroy_buffer(self.weight_down_pos_buffer, None);

            self.device.destroy_pipeline(self.pipeline_nda, None);
            self.device.destroy_pipeline(self.pipeline_act, None);
            self.device
                .destroy_pipeline_layout(self.pipeline_layout_nda, None);
            self.device
                .destroy_pipeline_layout(self.pipeline_layout_act, None);
            self.device
                .destroy_descriptor_set_layout(self.desc_set_layout_nda, None);
            self.device
                .destroy_descriptor_set_layout(self.desc_set_layout_act, None);

            self.device.destroy_shader_module(self.shader_nda, None);
            self.device.destroy_shader_module(self.shader_act, None);
        }
    }
}
