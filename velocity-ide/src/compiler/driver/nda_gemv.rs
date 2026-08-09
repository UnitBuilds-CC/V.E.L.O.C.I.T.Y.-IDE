use super::packing::*;
use super::vulkan_init::*;
use ash::vk;
use ash::Device;
use std::ffi::CString;
use std::time::Instant;

pub struct VulkanNdaGemv {
    pub device: Device,
    pub queue: vk::Queue,
    pub k: u32,
    pub n: u32,
    pub version: u32,

    pub shader_module: vk::ShaderModule,
    pub desc_set_layout: vk::DescriptorSetLayout,
    pub pipeline_layout: vk::PipelineLayout,
    pub compute_pipeline: vk::Pipeline,

    pub input_active_buffer: vk::Buffer,
    pub input_active_memory: vk::DeviceMemory,
    pub input_active_ptr: *mut std::ffi::c_void,

    pub input_pos_buffer: vk::Buffer,
    pub input_pos_memory: vk::DeviceMemory,
    pub input_pos_ptr: *mut std::ffi::c_void,

    pub weight_active_buffer: vk::Buffer,
    pub weight_active_memory: vk::DeviceMemory,

    pub weight_pos_buffer: vk::Buffer,
    pub weight_pos_memory: vk::DeviceMemory,

    pub output_buffer: vk::Buffer,
    pub output_memory: vk::DeviceMemory,
    pub output_ptr: *mut std::ffi::c_void,

    pub desc_pool: vk::DescriptorPool,
    pub desc_set: vk::DescriptorSet,
    pub command_pool: vk::CommandPool,
    pub command_buffer: vk::CommandBuffer,
    pub fence: vk::Fence,
}

impl VulkanNdaGemv {
    pub fn record_dispatch(&self, cmd: vk::CommandBuffer) {
        unsafe {
            self.device.cmd_bind_pipeline(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                self.compute_pipeline,
            );
            self.device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                self.pipeline_layout,
                0,
                &[self.desc_set],
                &[],
            );

            let params = [self.k, self.n];
            let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
            self.device.cmd_push_constants(
                cmd,
                self.pipeline_layout,
                vk::ShaderStageFlags::COMPUTE,
                0,
                params_bytes,
            );

            let workgroup_count_y = self.n.div_ceil(16);
            self.device.cmd_dispatch(cmd, 1, workgroup_count_y, 1);
        }
    }

    pub fn new(
        driver: &VulkanDriver,
        k: u32,
        n: u32,
        weight_bytes: &[u8],
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let device = driver.device.clone();
        let physical_device = driver.physical_device;
        let instance = driver.instance.clone();
        let queue = driver.compute_queue;

        let shader_info =
            vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::NDA_SPV);
        let shader_module = unsafe { device.create_shader_module(&shader_info, None)? };

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
        let stage_info = vk::PipelineShaderStageCreateInfo::builder()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(shader_module)
            .name(&main_entry);
        let pipeline_create_info = vk::ComputePipelineCreateInfo::builder()
            .stage(stage_info.build())
            .layout(pipeline_layout);
        let compute_pipelines = unsafe {
            device
                .create_compute_pipelines(
                    vk::PipelineCache::null(),
                    &[pipeline_create_info.build()],
                    None,
                )
                .map_err(|(_, e)| e)?
        };
        let compute_pipeline = compute_pipelines[0];

        let (active_w_bytes, pos_w_bytes) = pack_weights_nda(weight_bytes, k as usize, n as usize);

        let (input_active_buffer, input_active_memory, input_active_ptr) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            (k / 8) as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (input_pos_buffer, input_pos_memory, input_pos_ptr) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            (k / 8) as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;

        let (weight_active_buffer, weight_active_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            active_w_bytes.len() as vk::DeviceSize,
            &active_w_bytes,
        )?;
        let (weight_pos_buffer, weight_pos_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            pos_w_bytes.len() as vk::DeviceSize,
            &pos_w_bytes,
        )?;

        let (output_buffer, output_memory, output_ptr) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            (n * 4) as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;

        let pool_sizes = [vk::DescriptorPoolSize::builder()
            .ty(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(5)
            .build()];
        let pool_info = vk::DescriptorPoolCreateInfo::builder()
            .max_sets(1)
            .pool_sizes(&pool_sizes);
        let desc_pool = unsafe { device.create_descriptor_pool(&pool_info, None)? };

        let desc_set = unsafe {
            device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::builder()
                    .descriptor_pool(desc_pool)
                    .set_layouts(&[desc_set_layout]),
            )?[0]
        };

        let buffer_infos = [
            vk::DescriptorBufferInfo::builder()
                .buffer(input_active_buffer)
                .offset(0)
                .range((k / 8) as vk::DeviceSize)
                .build(),
            vk::DescriptorBufferInfo::builder()
                .buffer(input_pos_buffer)
                .offset(0)
                .range((k / 8) as vk::DeviceSize)
                .build(),
            vk::DescriptorBufferInfo::builder()
                .buffer(weight_active_buffer)
                .offset(0)
                .range(active_w_bytes.len() as vk::DeviceSize)
                .build(),
            vk::DescriptorBufferInfo::builder()
                .buffer(weight_pos_buffer)
                .offset(0)
                .range(pos_w_bytes.len() as vk::DeviceSize)
                .build(),
            vk::DescriptorBufferInfo::builder()
                .buffer(output_buffer)
                .offset(0)
                .range((n * 4) as vk::DeviceSize)
                .build(),
        ];
        let writes = [
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos[0..1])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set)
                .dst_binding(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos[1..2])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set)
                .dst_binding(2)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos[2..3])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set)
                .dst_binding(3)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos[3..4])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set)
                .dst_binding(4)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos[4..5])
                .build(),
        ];
        unsafe { device.update_descriptor_sets(&writes, &[]) };

        let command_pool_info = vk::CommandPoolCreateInfo::builder()
            .queue_family_index(driver.queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = unsafe { device.create_command_pool(&command_pool_info, None)? };
        let command_buffers = unsafe {
            device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::builder()
                    .command_pool(command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )?
        };
        let command_buffer = command_buffers[0];

        let begin_info = vk::CommandBufferBeginInfo::builder()
            .flags(vk::CommandBufferUsageFlags::SIMULTANEOUS_USE);
        unsafe {
            device.begin_command_buffer(command_buffer, &begin_info)?;
            device.cmd_bind_pipeline(
                command_buffer,
                vk::PipelineBindPoint::COMPUTE,
                compute_pipeline,
            );
            device.cmd_bind_descriptor_sets(
                command_buffer,
                vk::PipelineBindPoint::COMPUTE,
                pipeline_layout,
                0,
                &[desc_set],
                &[],
            );

            let params = [k, n];
            let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
            device.cmd_push_constants(
                command_buffer,
                pipeline_layout,
                vk::ShaderStageFlags::COMPUTE,
                0,
                params_bytes,
            );

            let workgroup_count = n.div_ceil(64);
            device.cmd_dispatch(command_buffer, workgroup_count, 1, 1);
            device.end_command_buffer(command_buffer)?;
        }

        let fence_info = vk::FenceCreateInfo::builder();
        let fence = unsafe { device.create_fence(&fence_info, None)? };

        Ok(Self {
            device,
            queue,
            k,
            n,
            version: 2,
            shader_module,
            desc_set_layout,
            pipeline_layout,
            compute_pipeline,
            input_active_buffer,
            input_active_memory,
            input_active_ptr,
            input_pos_buffer,
            input_pos_memory,
            input_pos_ptr,
            weight_active_buffer,
            weight_active_memory,
            weight_pos_buffer,
            weight_pos_memory,
            output_buffer,
            output_memory,
            output_ptr,
            desc_pool,
            desc_set,
            command_pool,
            command_buffer,
            fence,
        })
    }

    pub fn new_direct(
        driver: &VulkanDriver,
        version: u32,
        k: u32,
        n: u32,
        active_w_bytes: &[u8],
        pos_w_bytes: &[u8],
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let device = driver.device.clone();
        let physical_device = driver.physical_device;
        let instance = driver.instance.clone();
        let queue = driver.compute_queue;

        let is_float_act = version == 3 || version == 4;

        let spv_code = match version {
            3 => crate::compiler::shaders::FP4_SPV,
            4 => crate::compiler::shaders::FP2_SPV,
            _ => crate::compiler::shaders::NDA_SPV,
        };
        let shader_info = vk::ShaderModuleCreateInfo::builder().code(spv_code);
        let shader_module = unsafe { device.create_shader_module(&shader_info, None)? };

        let bindings = if is_float_act {
            vec![
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
            ]
        } else {
            vec![
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
            ]
        };
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
        let stage_info = vk::PipelineShaderStageCreateInfo::builder()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(shader_module)
            .name(&main_entry);
        let pipeline_create_info = vk::ComputePipelineCreateInfo::builder()
            .stage(stage_info.build())
            .layout(pipeline_layout);
        let compute_pipelines = unsafe {
            device
                .create_compute_pipelines(
                    vk::PipelineCache::null(),
                    &[pipeline_create_info.build()],
                    None,
                )
                .map_err(|(_, e)| e)?
        };
        let compute_pipeline = compute_pipelines[0];

        let input_active_size = if is_float_act {
            (k * 4) as vk::DeviceSize
        } else {
            (k / 8) as vk::DeviceSize
        };
        let (input_active_buffer, input_active_memory, input_active_ptr) = if is_float_act {
            (
                driver.shared_input_buffer,
                driver.shared_input_memory,
                driver.shared_input_ptr,
            )
        } else {
            create_coherent_buffer(
                &device,
                &instance,
                physical_device,
                input_active_size,
                vk::BufferUsageFlags::STORAGE_BUFFER,
            )?
        };

        let (input_pos_buffer, input_pos_memory, input_pos_ptr) = if is_float_act {
            create_coherent_buffer(
                &device,
                &instance,
                physical_device,
                4 as vk::DeviceSize,
                vk::BufferUsageFlags::STORAGE_BUFFER,
            )?
        } else {
            create_coherent_buffer(
                &device,
                &instance,
                physical_device,
                (k / 8) as vk::DeviceSize,
                vk::BufferUsageFlags::STORAGE_BUFFER,
            )?
        };

        let (weight_active_buffer, weight_active_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            active_w_bytes.len() as vk::DeviceSize,
            active_w_bytes,
        )?;
        let (weight_pos_buffer, weight_pos_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            pos_w_bytes.len() as vk::DeviceSize,
            pos_w_bytes,
        )?;

        let (output_buffer, output_memory, output_ptr) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            (n * 4) as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;

        let num_bindings = if is_float_act { 4 } else { 5 };
        let pool_sizes = [vk::DescriptorPoolSize::builder()
            .ty(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(num_bindings)
            .build()];
        let pool_info = vk::DescriptorPoolCreateInfo::builder()
            .max_sets(1)
            .pool_sizes(&pool_sizes);
        let desc_pool = unsafe { device.create_descriptor_pool(&pool_info, None)? };

        let desc_set = unsafe {
            device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::builder()
                    .descriptor_pool(desc_pool)
                    .set_layouts(&[desc_set_layout]),
            )?[0]
        };

        let buffer_infos = if is_float_act {
            vec![
                vk::DescriptorBufferInfo::builder()
                    .buffer(input_active_buffer)
                    .offset(0)
                    .range(input_active_size)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(weight_active_buffer)
                    .offset(0)
                    .range(active_w_bytes.len() as vk::DeviceSize)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(weight_pos_buffer)
                    .offset(0)
                    .range(pos_w_bytes.len() as vk::DeviceSize)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(output_buffer)
                    .offset(0)
                    .range((n * 4) as vk::DeviceSize)
                    .build(),
            ]
        } else {
            vec![
                vk::DescriptorBufferInfo::builder()
                    .buffer(input_active_buffer)
                    .offset(0)
                    .range((k / 8) as vk::DeviceSize)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(input_pos_buffer)
                    .offset(0)
                    .range((k / 8) as vk::DeviceSize)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(weight_active_buffer)
                    .offset(0)
                    .range(active_w_bytes.len() as vk::DeviceSize)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(weight_pos_buffer)
                    .offset(0)
                    .range(pos_w_bytes.len() as vk::DeviceSize)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(output_buffer)
                    .offset(0)
                    .range((n * 4) as vk::DeviceSize)
                    .build(),
            ]
        };

        let mut writes = Vec::new();
        for i in 0..num_bindings as usize {
            writes.push(
                vk::WriteDescriptorSet::builder()
                    .dst_set(desc_set)
                    .dst_binding(i as u32)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&buffer_infos[i..i + 1])
                    .build(),
            );
        }
        unsafe { device.update_descriptor_sets(&writes, &[]) };

        let command_pool_info = vk::CommandPoolCreateInfo::builder()
            .queue_family_index(driver.queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = unsafe { device.create_command_pool(&command_pool_info, None)? };
        let command_buffers = unsafe {
            device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::builder()
                    .command_pool(command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )?
        };
        let command_buffer = command_buffers[0];

        let begin_info = vk::CommandBufferBeginInfo::builder()
            .flags(vk::CommandBufferUsageFlags::SIMULTANEOUS_USE);
        unsafe {
            device.begin_command_buffer(command_buffer, &begin_info)?;
            device.cmd_bind_pipeline(
                command_buffer,
                vk::PipelineBindPoint::COMPUTE,
                compute_pipeline,
            );
            device.cmd_bind_descriptor_sets(
                command_buffer,
                vk::PipelineBindPoint::COMPUTE,
                pipeline_layout,
                0,
                &[desc_set],
                &[],
            );

            let params = [k, n];
            let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
            device.cmd_push_constants(
                command_buffer,
                pipeline_layout,
                vk::ShaderStageFlags::COMPUTE,
                0,
                params_bytes,
            );

            let workgroup_count_y = n.div_ceil(16);
            device.cmd_dispatch(command_buffer, 1, workgroup_count_y, 1);
            device.end_command_buffer(command_buffer)?;
        }

        let fence_info = vk::FenceCreateInfo::builder();
        let fence = unsafe { device.create_fence(&fence_info, None)? };

        Ok(Self {
            device,
            queue,
            k,
            n,
            version,
            shader_module,
            desc_set_layout,
            pipeline_layout,
            compute_pipeline,
            input_active_buffer,
            input_active_memory,
            input_active_ptr,
            input_pos_buffer,
            input_pos_memory,
            input_pos_ptr,
            weight_active_buffer,
            weight_active_memory,
            weight_pos_buffer,
            weight_pos_memory,
            output_buffer,
            output_memory,
            output_ptr,
            desc_pool,
            desc_set,
            command_pool,
            command_buffer,
            fence,
        })
    }

    pub fn run_float(
        &self,
        input_floats: &[f32],
        output_floats: &mut [f32],
    ) -> Result<f64, Box<dyn std::error::Error>> {
        unsafe {
            std::ptr::copy_nonoverlapping(
                input_floats.as_ptr(),
                self.input_active_ptr as *mut f32,
                input_floats.len(),
            );
        }

        let start = Instant::now();
        unsafe {
            self.device.reset_fences(&[self.fence])?;
            self.device.queue_submit(
                self.queue,
                &[vk::SubmitInfo::builder()
                    .command_buffers(&[self.command_buffer])
                    .build()],
                self.fence,
            )?;
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        let duration_us = start.elapsed().as_micros() as f64;

        unsafe {
            std::ptr::copy_nonoverlapping(
                self.output_ptr as *const f32,
                output_floats.as_mut_ptr(),
                output_floats.len(),
            );
        }

        Ok(duration_us)
    }

    pub fn submit_async_float(
        &self,
        input_floats: &[f32],
    ) -> Result<(), Box<dyn std::error::Error>> {
        unsafe {
            std::ptr::copy_nonoverlapping(
                input_floats.as_ptr(),
                self.input_active_ptr as *mut f32,
                input_floats.len(),
            );
            self.device.reset_fences(&[self.fence])?;
            self.device.queue_submit(
                self.queue,
                &[vk::SubmitInfo::builder()
                    .command_buffers(&[self.command_buffer])
                    .build()],
                self.fence,
            )?;
        }
        Ok(())
    }

    pub fn wait_and_copy_float(
        &self,
        output_floats: &mut [f32],
    ) -> Result<f64, Box<dyn std::error::Error>> {
        let start = Instant::now();
        unsafe {
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        let duration_us = start.elapsed().as_micros() as f64;

        unsafe {
            std::ptr::copy_nonoverlapping(
                self.output_ptr as *const f32,
                output_floats.as_mut_ptr(),
                output_floats.len(),
            );
        }

        Ok(duration_us)
    }

    pub fn submit_async_float_no_copy(&self) -> Result<(), Box<dyn std::error::Error>> {
        unsafe {
            self.device.reset_fences(&[self.fence])?;
            self.device.queue_submit(
                self.queue,
                &[vk::SubmitInfo::builder()
                    .command_buffers(&[self.command_buffer])
                    .build()],
                self.fence,
            )?;
        }
        Ok(())
    }

    pub fn run_float_no_copy(
        &self,
        output_floats: &mut [f32],
    ) -> Result<f64, Box<dyn std::error::Error>> {
        let start = Instant::now();
        unsafe {
            self.device.reset_fences(&[self.fence])?;
            self.device.queue_submit(
                self.queue,
                &[vk::SubmitInfo::builder()
                    .command_buffers(&[self.command_buffer])
                    .build()],
                self.fence,
            )?;
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        let duration_us = start.elapsed().as_micros() as f64;

        unsafe {
            std::ptr::copy_nonoverlapping(
                self.output_ptr as *const f32,
                output_floats.as_mut_ptr(),
                output_floats.len(),
            );
        }

        Ok(duration_us)
    }

    pub fn run(
        &self,
        input_active: &[u8],
        input_pos: &[u8],
        output_floats: &mut [f32],
    ) -> Result<f64, Box<dyn std::error::Error>> {
        unsafe {
            std::ptr::copy_nonoverlapping(
                input_active.as_ptr(),
                self.input_active_ptr as *mut u8,
                input_active.len(),
            );
            std::ptr::copy_nonoverlapping(
                input_pos.as_ptr(),
                self.input_pos_ptr as *mut u8,
                input_pos.len(),
            );
        }

        let start = Instant::now();
        unsafe {
            self.device.reset_fences(&[self.fence])?;
            self.device.queue_submit(
                self.queue,
                &[vk::SubmitInfo::builder()
                    .command_buffers(&[self.command_buffer])
                    .build()],
                self.fence,
            )?;
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        let duration_us = start.elapsed().as_micros() as f64;

        unsafe {
            std::ptr::copy_nonoverlapping(
                self.output_ptr as *const f32,
                output_floats.as_mut_ptr(),
                output_floats.len(),
            );
        }

        Ok(duration_us)
    }
}

impl Drop for VulkanNdaGemv {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_descriptor_pool(self.desc_pool, None);

            let destroy_buffer =
                |device: &Device, buffer: vk::Buffer, memory: vk::DeviceMemory, mapped: bool| {
                    if mapped {
                        device.unmap_memory(memory);
                    }
                    device.free_memory(memory, None);
                    device.destroy_buffer(buffer, None);
                };

            let is_float_act = self.version == 3 || self.version == 4;
            if !is_float_act {
                destroy_buffer(
                    &self.device,
                    self.input_active_buffer,
                    self.input_active_memory,
                    true,
                );
            }
            destroy_buffer(
                &self.device,
                self.input_pos_buffer,
                self.input_pos_memory,
                true,
            );
            destroy_buffer(
                &self.device,
                self.weight_active_buffer,
                self.weight_active_memory,
                false,
            );
            destroy_buffer(
                &self.device,
                self.weight_pos_buffer,
                self.weight_pos_memory,
                false,
            );
            destroy_buffer(&self.device, self.output_buffer, self.output_memory, true);

            self.device.destroy_pipeline(self.compute_pipeline, None);
            self.device
                .destroy_pipeline_layout(self.pipeline_layout, None);
            self.device
                .destroy_descriptor_set_layout(self.desc_set_layout, None);
            self.device.destroy_shader_module(self.shader_module, None);
        }
    }
}
