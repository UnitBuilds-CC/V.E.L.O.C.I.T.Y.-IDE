#![allow(dead_code)]

use super::packing::*;
use super::vulkan_init::*;
use ash::vk;
use ash::Device;
use std::ffi::CString;
use std::time::Instant;

pub struct VulkanGemv {
    pub device: Device,
    pub queue: vk::Queue,
    pub k: u32,
    pub n: u32,
    pub is_ternary: bool,

    pub shader_module: vk::ShaderModule,
    pub desc_set_layout: vk::DescriptorSetLayout,
    pub pipeline_layout: vk::PipelineLayout,
    pub compute_pipeline: vk::Pipeline,

    pub input_buffer: vk::Buffer,
    pub input_memory: vk::DeviceMemory,
    pub input_ptr: *mut std::ffi::c_void,
    pub input_size: vk::DeviceSize,

    pub weight_buffer: vk::Buffer,
    pub weight_memory: vk::DeviceMemory,
    pub weight_ptr: *mut std::ffi::c_void,
    pub weight_size: vk::DeviceSize,

    pub output_buffer: vk::Buffer,
    pub output_memory: vk::DeviceMemory,
    pub output_ptr: *mut std::ffi::c_void,
    pub output_size: vk::DeviceSize,

    pub desc_pool: vk::DescriptorPool,
    pub desc_set: vk::DescriptorSet,

    pub command_pool: vk::CommandPool,
    pub command_buffer: vk::CommandBuffer,
    pub fence: vk::Fence,
}

impl VulkanGemv {
    pub fn new(
        driver: &VulkanDriver,
        is_ternary: bool,
        k: u32,
        n: u32,
        weight_bytes: &[u8],
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let device = driver.device.clone();
        let physical_device = driver.physical_device;
        let instance = driver.instance.clone();
        let queue = driver.compute_queue;

        let spv_code = if is_ternary {
            crate::compiler::shaders::TERNARY_SPV
        } else {
            crate::compiler::shaders::INT4_SPV
        };
        let shader_info = vk::ShaderModuleCreateInfo::builder().code(spv_code);
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

        let input_size = if is_ternary {
            ((k / 16) * 4) as vk::DeviceSize
        } else {
            (k * 4) as vk::DeviceSize
        };
        let weight_size = weight_bytes.len() as vk::DeviceSize;
        let output_size = (n * 4) as vk::DeviceSize;

        let (input_buffer, input_memory, input_ptr) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            input_size,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (weight_buffer, weight_memory) = if is_ternary {
            let packed = pack_weights_uvec4(weight_bytes, k as usize, n as usize);
            create_device_local_buffer(
                &device,
                &instance,
                physical_device,
                queue,
                driver.queue_family_index,
                weight_size,
                &packed,
            )?
        } else {
            create_device_local_buffer(
                &device,
                &instance,
                physical_device,
                queue,
                driver.queue_family_index,
                weight_size,
                weight_bytes,
            )?
        };
        let (output_buffer, output_memory, output_ptr) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            output_size,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;

        let pool_sizes = [vk::DescriptorPoolSize::builder()
            .ty(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(3)
            .build()];
        let pool_info = vk::DescriptorPoolCreateInfo::builder()
            .max_sets(1)
            .pool_sizes(&pool_sizes);
        let desc_pool = unsafe { device.create_descriptor_pool(&pool_info, None)? };

        let layouts = [desc_set_layout];
        let alloc_info = vk::DescriptorSetAllocateInfo::builder()
            .descriptor_pool(desc_pool)
            .set_layouts(&layouts);
        let desc_sets = unsafe { device.allocate_descriptor_sets(&alloc_info)? };
        let desc_set = desc_sets[0];

        let buffer_infos = [
            vk::DescriptorBufferInfo::builder()
                .buffer(input_buffer)
                .offset(0)
                .range(input_size)
                .build(),
            vk::DescriptorBufferInfo::builder()
                .buffer(weight_buffer)
                .offset(0)
                .range(weight_size)
                .build(),
            vk::DescriptorBufferInfo::builder()
                .buffer(output_buffer)
                .offset(0)
                .range(output_size)
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
        ];
        unsafe { device.update_descriptor_sets(&writes, &[]) };

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

            let workgroup_count = if is_ternary {
                n.div_ceil(256)
            } else {
                n.div_ceil(64)
            };
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
            is_ternary,
            shader_module,
            desc_set_layout,
            pipeline_layout,
            compute_pipeline,
            input_buffer,
            input_memory,
            input_ptr,
            input_size,
            weight_buffer,
            weight_memory,
            weight_ptr: std::ptr::null_mut(),
            weight_size,
            output_buffer,
            output_memory,
            output_ptr,
            output_size,
            desc_pool,
            desc_set,
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
                self.input_ptr as *mut u8,
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
                self.output_ptr as *const f32,
                output_floats.as_mut_ptr(),
                output_floats.len(),
            );
        }

        Ok(duration_us)
    }
}

impl Drop for VulkanGemv {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_descriptor_pool(self.desc_pool, None);

            self.device.unmap_memory(self.input_memory);
            self.device.free_memory(self.input_memory, None);
            self.device.destroy_buffer(self.input_buffer, None);

            self.device.free_memory(self.weight_memory, None);
            self.device.destroy_buffer(self.weight_buffer, None);

            self.device.unmap_memory(self.output_memory);
            self.device.free_memory(self.output_memory, None);
            self.device.destroy_buffer(self.output_buffer, None);

            self.device.destroy_pipeline(self.compute_pipeline, None);
            self.device
                .destroy_pipeline_layout(self.pipeline_layout, None);
            self.device
                .destroy_descriptor_set_layout(self.desc_set_layout, None);
            self.device.destroy_shader_module(self.shader_module, None);
        }
    }
}
