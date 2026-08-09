use super::packing::*;
use super::vulkan_init::*;
use ash::vk;
use ash::Device;
use std::ffi::CString;
use std::time::Instant;

pub struct VulkanBitNetLayer {
    pub device: Device,
    pub queue: vk::Queue,

    pub shader_ternary: vk::ShaderModule,
    pub shader_act: vk::ShaderModule,

    pub desc_set_layout: vk::DescriptorSetLayout,
    pub pipeline_layout: vk::PipelineLayout,

    pub pipeline_ternary: vk::Pipeline,
    pub pipeline_act: vk::Pipeline,

    pub inputs_3200_buffer: vk::Buffer,
    pub inputs_3200_memory: vk::DeviceMemory,
    pub inputs_3200_ptr: *mut std::ffi::c_void,

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

    pub inputs_8640_buffer: vk::Buffer,
    pub inputs_8640_memory: vk::DeviceMemory,

    pub out_3200_down_buffer: vk::Buffer,
    pub out_3200_down_memory: vk::DeviceMemory,
    pub out_3200_down_ptr: *mut std::ffi::c_void,

    pub weight_q_buffer: vk::Buffer,
    pub weight_q_memory: vk::DeviceMemory,
    pub weight_k_buffer: vk::Buffer,
    pub weight_k_memory: vk::DeviceMemory,
    pub weight_v_buffer: vk::Buffer,
    pub weight_v_memory: vk::DeviceMemory,
    pub weight_o_buffer: vk::Buffer,
    pub weight_o_memory: vk::DeviceMemory,
    pub weight_gate_buffer: vk::Buffer,
    pub weight_gate_memory: vk::DeviceMemory,
    pub weight_up_buffer: vk::Buffer,
    pub weight_up_memory: vk::DeviceMemory,
    pub weight_down_buffer: vk::Buffer,
    pub weight_down_memory: vk::DeviceMemory,

    pub desc_pool: vk::DescriptorPool,
    pub desc_sets: Vec<vk::DescriptorSet>,

    pub command_pool: vk::CommandPool,
    pub command_buffer: vk::CommandBuffer,
    pub fence: vk::Fence,
}

impl VulkanBitNetLayer {
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

        let shader_info_ternary =
            vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::TERNARY_SPV);
        let shader_ternary = unsafe { device.create_shader_module(&shader_info_ternary, None)? };

        let shader_info_act =
            vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::ACT_BITNET_SPV);
        let shader_act = unsafe { device.create_shader_module(&shader_info_act, None)? };

        let bindings = [
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
        ];
        let layout_info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);
        let desc_set_layout = unsafe { device.create_descriptor_set_layout(&layout_info, None)? };

        let push_constant_ranges = [vk::PushConstantRange::builder()
            .stage_flags(vk::ShaderStageFlags::COMPUTE)
            .offset(0)
            .size(8)
            .build()];
        let layouts = [desc_set_layout];
        let pipeline_layout_info = vk::PipelineLayoutCreateInfo::builder()
            .set_layouts(&layouts)
            .push_constant_ranges(&push_constant_ranges);
        let pipeline_layout =
            unsafe { device.create_pipeline_layout(&pipeline_layout_info, None)? };

        let main_entry = CString::new("main")?;

        let stage_info_ternary = vk::PipelineShaderStageCreateInfo::builder()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(shader_ternary)
            .name(&main_entry);
        let pipeline_create_info_ternary = vk::ComputePipelineCreateInfo::builder()
            .stage(stage_info_ternary.build())
            .layout(pipeline_layout);

        let stage_info_act = vk::PipelineShaderStageCreateInfo::builder()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(shader_act)
            .name(&main_entry);
        let pipeline_create_info_act = vk::ComputePipelineCreateInfo::builder()
            .stage(stage_info_act.build())
            .layout(pipeline_layout);

        let pipelines = unsafe {
            device
                .create_compute_pipelines(
                    vk::PipelineCache::null(),
                    &[
                        pipeline_create_info_ternary.build(),
                        pipeline_create_info_act.build(),
                    ],
                    None,
                )
                .map_err(|(_, e)| e)?
        };
        let pipeline_ternary = pipelines[0];
        let pipeline_act = pipelines[1];

        let (inputs_3200_buffer, inputs_3200_memory, inputs_3200_ptr) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            (3200 / 16) * 4,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (out_3200_down_buffer, out_3200_down_memory, out_3200_down_ptr) =
            create_coherent_buffer(
                &device,
                &instance,
                physical_device,
                3200 * 4,
                vk::BufferUsageFlags::STORAGE_BUFFER,
            )?;

        let (out_3200_q_buffer, out_3200_q_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            3200 * 4,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (out_3200_k_buffer, out_3200_k_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            3200 * 4,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (out_3200_v_buffer, out_3200_v_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            3200 * 4,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (out_3200_o_buffer, out_3200_o_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            3200 * 4,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;

        let (out_8640_gate_buffer, out_8640_gate_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            8640 * 4,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (out_8640_up_buffer, out_8640_up_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            8640 * 4,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (inputs_8640_buffer, inputs_8640_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            (8640 / 16) * 4,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;

        let (weight_q_buffer, weight_q_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            weight_q.len() as vk::DeviceSize,
            &pack_weights_uvec4(weight_q, 3200, 3200),
        )?;
        let (weight_k_buffer, weight_k_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            weight_k.len() as vk::DeviceSize,
            &pack_weights_uvec4(weight_k, 3200, 3200),
        )?;
        let (weight_v_buffer, weight_v_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            weight_v.len() as vk::DeviceSize,
            &pack_weights_uvec4(weight_v, 3200, 3200),
        )?;
        let (weight_o_buffer, weight_o_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            weight_o.len() as vk::DeviceSize,
            &pack_weights_uvec4(weight_o, 3200, 3200),
        )?;
        let (weight_gate_buffer, weight_gate_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            weight_gate.len() as vk::DeviceSize,
            &pack_weights_uvec4(weight_gate, 3200, 8640),
        )?;
        let (weight_up_buffer, weight_up_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            weight_up.len() as vk::DeviceSize,
            &pack_weights_uvec4(weight_up, 3200, 8640),
        )?;
        let (weight_down_buffer, weight_down_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            weight_down.len() as vk::DeviceSize,
            &pack_weights_uvec4(weight_down, 8640, 3200),
        )?;

        let pool_sizes = [vk::DescriptorPoolSize::builder()
            .ty(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(24)
            .build()];
        let pool_info = vk::DescriptorPoolCreateInfo::builder()
            .max_sets(8)
            .pool_sizes(&pool_sizes);
        let desc_pool = unsafe { device.create_descriptor_pool(&pool_info, None)? };

        let layouts_vec = vec![desc_set_layout; 8];
        let alloc_info = vk::DescriptorSetAllocateInfo::builder()
            .descriptor_pool(desc_pool)
            .set_layouts(&layouts_vec);
        let desc_sets = unsafe { device.allocate_descriptor_sets(&alloc_info)? };

        let set_configs = [
            (inputs_3200_buffer, weight_q_buffer, out_3200_q_buffer),
            (inputs_3200_buffer, weight_k_buffer, out_3200_k_buffer),
            (inputs_3200_buffer, weight_v_buffer, out_3200_v_buffer),
            (inputs_3200_buffer, weight_o_buffer, out_3200_o_buffer),
            (inputs_3200_buffer, weight_gate_buffer, out_8640_gate_buffer),
            (inputs_3200_buffer, weight_up_buffer, out_8640_up_buffer),
            (out_8640_gate_buffer, out_8640_up_buffer, inputs_8640_buffer),
            (inputs_8640_buffer, weight_down_buffer, out_3200_down_buffer),
        ];

        for (i, (b0, b1, b2)) in set_configs.iter().enumerate() {
            let buffer_infos = [
                vk::DescriptorBufferInfo::builder()
                    .buffer(*b0)
                    .offset(0)
                    .range(vk::WHOLE_SIZE)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(*b1)
                    .offset(0)
                    .range(vk::WHOLE_SIZE)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(*b2)
                    .offset(0)
                    .range(vk::WHOLE_SIZE)
                    .build(),
            ];
            let writes = [
                vk::WriteDescriptorSet::builder()
                    .dst_set(desc_sets[i])
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&buffer_infos[0..1])
                    .build(),
                vk::WriteDescriptorSet::builder()
                    .dst_set(desc_sets[i])
                    .dst_binding(1)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&buffer_infos[1..2])
                    .build(),
                vk::WriteDescriptorSet::builder()
                    .dst_set(desc_sets[i])
                    .dst_binding(2)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&buffer_infos[2..3])
                    .build(),
            ];
            unsafe { device.update_descriptor_sets(&writes, &[]) };
        }

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

            let dispatch_ternary =
                |cmd: vk::CommandBuffer, set: vk::DescriptorSet, k: u32, n: u32| {
                    device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, pipeline_ternary);
                    device.cmd_bind_descriptor_sets(
                        cmd,
                        vk::PipelineBindPoint::COMPUTE,
                        pipeline_layout,
                        0,
                        &[set],
                        &[],
                    );
                    let params = [k, n];
                    let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
                    device.cmd_push_constants(
                        cmd,
                        pipeline_layout,
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
                    pipeline_layout,
                    0,
                    &[set],
                    &[],
                );
                let params = [n, 0u32];
                let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
                device.cmd_push_constants(
                    cmd,
                    pipeline_layout,
                    vk::ShaderStageFlags::COMPUTE,
                    0,
                    params_bytes,
                );
                let workgroups = n.div_ceil(256u32);
                device.cmd_dispatch(cmd, workgroups, 1, 1);
            };

            dispatch_ternary(command_buffer, desc_sets[0], 3200, 3200);
            dispatch_ternary(command_buffer, desc_sets[1], 3200, 3200);
            dispatch_ternary(command_buffer, desc_sets[2], 3200, 3200);
            dispatch_ternary(command_buffer, desc_sets[3], 3200, 3200);
            dispatch_ternary(command_buffer, desc_sets[4], 3200, 8640);
            dispatch_ternary(command_buffer, desc_sets[5], 3200, 8640);
            dispatch_act(command_buffer, desc_sets[6], 8640);
            dispatch_ternary(command_buffer, desc_sets[7], 8640, 3200);

            device.end_command_buffer(command_buffer)?;
        }

        let fence_info = vk::FenceCreateInfo::builder();
        let fence = unsafe { device.create_fence(&fence_info, None)? };

        Ok(Self {
            device,
            queue,
            shader_ternary,
            shader_act,
            desc_set_layout,
            pipeline_layout,
            pipeline_ternary,
            pipeline_act,
            inputs_3200_buffer,
            inputs_3200_memory,
            inputs_3200_ptr,
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
            inputs_8640_buffer,
            inputs_8640_memory,
            out_3200_down_buffer,
            out_3200_down_memory,
            out_3200_down_ptr,
            weight_q_buffer,
            weight_q_memory,
            weight_k_buffer,
            weight_k_memory,
            weight_v_buffer,
            weight_v_memory,
            weight_o_buffer,
            weight_o_memory,
            weight_gate_buffer,
            weight_gate_memory,
            weight_up_buffer,
            weight_up_memory,
            weight_down_buffer,
            weight_down_memory,
            desc_pool,
            desc_sets,
            command_pool,
            command_buffer,
            fence,
        })
    }

    pub fn run(
        &self,
        input_bytes: &[u8],
        output_floats: &mut [f32],
    ) -> Result<f64, Box<dyn std::error::Error>> {
        unsafe {
            std::ptr::copy_nonoverlapping(
                input_bytes.as_ptr(),
                self.inputs_3200_ptr as *mut u8,
                input_bytes.len(),
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

impl Drop for VulkanBitNetLayer {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_descriptor_pool(self.desc_pool, None);

            self.device.unmap_memory(self.inputs_3200_memory);
            self.device.free_memory(self.inputs_3200_memory, None);
            self.device.destroy_buffer(self.inputs_3200_buffer, None);

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

            self.device.free_memory(self.inputs_8640_memory, None);
            self.device.destroy_buffer(self.inputs_8640_buffer, None);

            self.device.free_memory(self.weight_q_memory, None);
            self.device.destroy_buffer(self.weight_q_buffer, None);

            self.device.free_memory(self.weight_k_memory, None);
            self.device.destroy_buffer(self.weight_k_buffer, None);

            self.device.free_memory(self.weight_v_memory, None);
            self.device.destroy_buffer(self.weight_v_buffer, None);

            self.device.free_memory(self.weight_o_memory, None);
            self.device.destroy_buffer(self.weight_o_buffer, None);

            self.device.free_memory(self.weight_gate_memory, None);
            self.device.destroy_buffer(self.weight_gate_buffer, None);

            self.device.free_memory(self.weight_up_memory, None);
            self.device.destroy_buffer(self.weight_up_buffer, None);

            self.device.free_memory(self.weight_down_memory, None);
            self.device.destroy_buffer(self.weight_down_buffer, None);

            self.device.destroy_pipeline(self.pipeline_ternary, None);
            self.device.destroy_pipeline(self.pipeline_act, None);
            self.device
                .destroy_pipeline_layout(self.pipeline_layout, None);
            self.device
                .destroy_descriptor_set_layout(self.desc_set_layout, None);

            self.device.destroy_shader_module(self.shader_ternary, None);
            self.device.destroy_shader_module(self.shader_act, None);
        }
    }
}
