use super::vulkan_init::*;
use ash::vk;
use ash::Device;
use std::ffi::CString;
use std::time::Instant;

pub struct VulkanQwenLayer {
    pub device: Device,
    pub queue: vk::Queue,

    pub shader_int4: vk::ShaderModule,
    pub shader_act: vk::ShaderModule,

    pub desc_set_layout: vk::DescriptorSetLayout,
    pub pipeline_layout: vk::PipelineLayout,

    pub pipeline_int4: vk::Pipeline,
    pub pipeline_act: vk::Pipeline,

    pub inputs_2304_buffer: vk::Buffer,
    pub inputs_2304_memory: vk::DeviceMemory,
    pub inputs_2304_ptr: *mut std::ffi::c_void,

    pub out_2304_a_buffer: vk::Buffer,
    pub out_2304_a_memory: vk::DeviceMemory,
    pub out_2304_a_ptr: *mut std::ffi::c_void,

    pub out_2304_b_buffer: vk::Buffer,
    pub out_2304_b_memory: vk::DeviceMemory,

    pub out_256_k_buffer: vk::Buffer,
    pub out_256_k_memory: vk::DeviceMemory,

    pub out_256_v_buffer: vk::Buffer,
    pub out_256_v_memory: vk::DeviceMemory,

    pub out_11008_gate_buffer: vk::Buffer,
    pub out_11008_gate_memory: vk::DeviceMemory,

    pub out_11008_up_buffer: vk::Buffer,
    pub out_11008_up_memory: vk::DeviceMemory,

    pub inputs_11008_buffer: vk::Buffer,
    pub inputs_11008_memory: vk::DeviceMemory,

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

impl VulkanQwenLayer {
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

        let shader_info_int4 =
            vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::INT4_SPV);
        let shader_int4 = unsafe { device.create_shader_module(&shader_info_int4, None)? };

        let shader_info_act =
            vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::ACT_QWEN_SPV);
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

        let stage_info_int4 = vk::PipelineShaderStageCreateInfo::builder()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(shader_int4)
            .name(&main_entry);
        let pipeline_create_info_int4 = vk::ComputePipelineCreateInfo::builder()
            .stage(stage_info_int4.build())
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
                        pipeline_create_info_int4.build(),
                        pipeline_create_info_act.build(),
                    ],
                    None,
                )
                .map_err(|(_, e)| e)?
        };
        let pipeline_int4 = pipelines[0];
        let pipeline_act = pipelines[1];

        let (inputs_2304_buffer, inputs_2304_memory, inputs_2304_ptr) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            2304 * 4,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (out_2304_a_buffer, out_2304_a_memory, out_2304_a_ptr) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            2304 * 4,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;

        let (out_2304_b_buffer, out_2304_b_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            2304 * 4,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (out_256_k_buffer, out_256_k_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            256 * 4,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (out_256_v_buffer, out_256_v_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            256 * 4,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (out_11008_gate_buffer, out_11008_gate_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            11008 * 4,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (out_11008_up_buffer, out_11008_up_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            11008 * 4,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (inputs_11008_buffer, inputs_11008_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            11008 * 4,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;

        let (weight_q_buffer, weight_q_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            weight_q.len() as vk::DeviceSize,
            weight_q,
        )?;
        let (weight_k_buffer, weight_k_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            weight_k.len() as vk::DeviceSize,
            weight_k,
        )?;
        let (weight_v_buffer, weight_v_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            weight_v.len() as vk::DeviceSize,
            weight_v,
        )?;
        let (weight_o_buffer, weight_o_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            weight_o.len() as vk::DeviceSize,
            weight_o,
        )?;
        let (weight_gate_buffer, weight_gate_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            weight_gate.len() as vk::DeviceSize,
            weight_gate,
        )?;
        let (weight_up_buffer, weight_up_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            weight_up.len() as vk::DeviceSize,
            weight_up,
        )?;
        let (weight_down_buffer, weight_down_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            weight_down.len() as vk::DeviceSize,
            weight_down,
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
            (inputs_2304_buffer, weight_q_buffer, out_2304_a_buffer),
            (inputs_2304_buffer, weight_k_buffer, out_256_k_buffer),
            (inputs_2304_buffer, weight_v_buffer, out_256_v_buffer),
            (inputs_2304_buffer, weight_o_buffer, out_2304_b_buffer),
            (inputs_2304_buffer, weight_gate_buffer, out_11008_gate_buffer),
            (inputs_2304_buffer, weight_up_buffer, out_11008_up_buffer),
            (
                out_11008_gate_buffer,
                out_11008_up_buffer,
                inputs_11008_buffer,
            ),
            (
                inputs_11008_buffer,
                weight_down_buffer,
                out_2304_a_buffer,
            ),
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

            let dispatch_int4 = |cmd: vk::CommandBuffer, set: vk::DescriptorSet, k: u32, n: u32| {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, pipeline_int4);
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
                let workgroups = n.div_ceil(64u32);
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
                let workgroups = n.div_ceil(64u32);
                device.cmd_dispatch(cmd, workgroups, 1, 1);
            };

            dispatch_int4(command_buffer, desc_sets[0], 2304, 2304);
            dispatch_int4(command_buffer, desc_sets[1], 2304, 256);
            dispatch_int4(command_buffer, desc_sets[2], 2304, 256);
            dispatch_int4(command_buffer, desc_sets[3], 2304, 2304);
            dispatch_int4(command_buffer, desc_sets[4], 2304, 11008);
            dispatch_int4(command_buffer, desc_sets[5], 2304, 11008);
            dispatch_act(command_buffer, desc_sets[6], 11008);
            dispatch_int4(command_buffer, desc_sets[7], 11008, 2304);

            device.end_command_buffer(command_buffer)?;
        }

        let fence_info = vk::FenceCreateInfo::builder();
        let fence = unsafe { device.create_fence(&fence_info, None)? };

        Ok(Self {
            device,
            queue,
            shader_int4,
            shader_act,
            desc_set_layout,
            pipeline_layout,
            pipeline_int4,
            pipeline_act,
            inputs_2304_buffer,
            inputs_2304_memory,
            inputs_2304_ptr,
            out_2304_a_buffer,
            out_2304_a_memory,
            out_2304_a_ptr,
            out_2304_b_buffer,
            out_2304_b_memory,
            out_256_k_buffer,
            out_256_k_memory,
            out_256_v_buffer,
            out_256_v_memory,
            out_11008_gate_buffer,
            out_11008_gate_memory,
            out_11008_up_buffer,
            out_11008_up_memory,
            inputs_11008_buffer,
            inputs_11008_memory,
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
                self.inputs_2304_ptr as *mut u8,
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
                self.out_2304_a_ptr as *const f32,
                output_floats.as_mut_ptr(),
                output_floats.len(),
            );
        }

        Ok(duration_us)
    }
}

impl Drop for VulkanQwenLayer {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_descriptor_pool(self.desc_pool, None);

            self.device.unmap_memory(self.inputs_2304_memory);
            self.device.free_memory(self.inputs_2304_memory, None);
            self.device.destroy_buffer(self.inputs_2304_buffer, None);

            self.device.unmap_memory(self.out_2304_a_memory);
            self.device.free_memory(self.out_2304_a_memory, None);
            self.device.destroy_buffer(self.out_2304_a_buffer, None);

            self.device.free_memory(self.out_2304_b_memory, None);
            self.device.destroy_buffer(self.out_2304_b_buffer, None);

            self.device.free_memory(self.out_256_k_memory, None);
            self.device.destroy_buffer(self.out_256_k_buffer, None);

            self.device.free_memory(self.out_256_v_memory, None);
            self.device.destroy_buffer(self.out_256_v_buffer, None);

            self.device.free_memory(self.out_11008_gate_memory, None);
            self.device
                .destroy_buffer(self.out_11008_gate_buffer, None);

            self.device.free_memory(self.out_11008_up_memory, None);
            self.device.destroy_buffer(self.out_11008_up_buffer, None);

            self.device.free_memory(self.inputs_11008_memory, None);
            self.device.destroy_buffer(self.inputs_11008_buffer, None);

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

            self.device.destroy_pipeline(self.pipeline_int4, None);
            self.device.destroy_pipeline(self.pipeline_act, None);
            self.device
                .destroy_pipeline_layout(self.pipeline_layout, None);
            self.device
                .destroy_descriptor_set_layout(self.desc_set_layout, None);

            self.device.destroy_shader_module(self.shader_int4, None);
            self.device.destroy_shader_module(self.shader_act, None);
        }
    }
}
