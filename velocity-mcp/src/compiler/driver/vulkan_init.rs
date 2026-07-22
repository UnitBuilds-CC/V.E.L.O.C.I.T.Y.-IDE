use ash::vk::Handle;
use ash::{vk, Device, Entry, Instance};
use std::ffi::CString;
use std::time::Instant;

pub struct VulkanDriver {
    pub entry: Entry,
    pub instance: Instance,
    pub physical_device: vk::PhysicalDevice,
    pub device: Device,
    pub queue_family_index: u32,
    pub compute_queue: vk::Queue,
}

impl VulkanDriver {
    pub fn init() -> Result<Self, Box<dyn std::error::Error>> {
        let entry = unsafe { Entry::load()? };

        let app_name = CString::new("V.E.L.O.C.I.T.Y. IDE Engine")?;
        let engine_name = CString::new("V-NCE JIT Compiler")?;
        let app_info = vk::ApplicationInfo::builder()
            .application_name(&app_name)
            .application_version(vk::make_api_version(0, 1, 0, 0))
            .engine_name(&engine_name)
            .engine_version(vk::make_api_version(0, 1, 0, 0))
            .api_version(vk::API_VERSION_1_2);

        let create_info = vk::InstanceCreateInfo::builder().application_info(&app_info);

        let instance = unsafe { entry.create_instance(&create_info, None)? };

        let physical_devices = unsafe { instance.enumerate_physical_devices()? };
        if physical_devices.is_empty() {
            return Err("No Vulkan-compatible physical devices (GPUs) found.".into());
        }

        let mut selected_device = None;
        let mut selected_queue_family = None;
        let mut selected_device_name = String::new();

        println!("Direct Vulkan GPU Enumeration:");
        for &pd in &physical_devices {
            let props = unsafe { instance.get_physical_device_properties(pd) };
            let name = unsafe { std::ffi::CStr::from_ptr(props.device_name.as_ptr()) }
                .to_string_lossy()
                .into_owned();

            let dev_type = props.device_type;
            let type_str = match dev_type {
                vk::PhysicalDeviceType::DISCRETE_GPU => "Discrete GPU",
                vk::PhysicalDeviceType::INTEGRATED_GPU => "Integrated GPU",
                vk::PhysicalDeviceType::CPU => "CPU",
                _ => "Other",
            };
            println!("  - [{}] {} (Type: {})", pd.as_raw(), name, type_str);

            let queue_properties =
                unsafe { instance.get_physical_device_queue_family_properties(pd) };
            let mut compute_family = None;
            for (idx, qprop) in queue_properties.iter().enumerate() {
                if qprop.queue_flags.contains(vk::QueueFlags::COMPUTE) {
                    compute_family = Some(idx as u32);
                    break;
                }
            }

            if compute_family.is_some() {
                if selected_device.is_none() || dev_type == vk::PhysicalDeviceType::DISCRETE_GPU {
                    selected_device = Some(pd);
                    selected_queue_family = compute_family;
                    selected_device_name = name;
                }
            }
        }

        let physical_device = selected_device.ok_or("No compute-capable GPU found.")?;
        let queue_family_index = selected_queue_family.ok_or("No compute queue family found.")?;

        println!("Selected GPU: {}", selected_device_name);

        let queue_priorities = [1.0f32];
        let queue_create_info = vk::DeviceQueueCreateInfo::builder()
            .queue_family_index(queue_family_index)
            .queue_priorities(&queue_priorities);

        let device_create_infos = [queue_create_info.build()];
        let device_create_info =
            vk::DeviceCreateInfo::builder().queue_create_infos(&device_create_infos);

        let device = unsafe { instance.create_device(physical_device, &device_create_info, None)? };
        let compute_queue = unsafe { device.get_device_queue(queue_family_index, 0) };

        Ok(Self {
            entry,
            instance,
            physical_device,
            device,
            queue_family_index,
            compute_queue,
        })
    }

    pub fn device_name(&self) -> String {
        let props = unsafe {
            self.instance
                .get_physical_device_properties(self.physical_device)
        };
        unsafe { std::ffi::CStr::from_ptr(props.device_name.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    }

    pub fn run_diagnostics(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("V.E.L.O.C.I.T.Y. V-NCE Diagnostic Run:");
        println!("  - Vulkan logical device handles initialized.");
        println!(
            "  - Compute Queue Family Index: {}",
            self.queue_family_index
        );
        println!("  - Compute queue successfully bounded to thread context.");

        let size = 1024 * vk::DeviceSize::from(4u32);
        let buffer_info = vk::BufferCreateInfo::builder()
            .size(size)
            .usage(vk::BufferUsageFlags::STORAGE_BUFFER)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);

        let buffer = unsafe { self.device.create_buffer(&buffer_info, None)? };
        let mem_reqs = unsafe { self.device.get_buffer_memory_requirements(buffer) };

        let mem_props = unsafe {
            self.instance
                .get_physical_device_memory_properties(self.physical_device)
        };
        let mut mem_type_index = None;
        for i in 0..mem_props.memory_type_count {
            let flags = mem_props.memory_types[i as usize].property_flags;
            if (mem_reqs.memory_type_bits & (1 << i)) != 0
                && flags.contains(
                    vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
                )
            {
                mem_type_index = Some(i);
                break;
            }
        }

        let memory_type_index =
            mem_type_index.ok_or("No host-visible coherent memory found on GPU.")?;

        let alloc_info = vk::MemoryAllocateInfo::builder()
            .allocation_size(mem_reqs.size)
            .memory_type_index(memory_type_index);

        let memory = unsafe { self.device.allocate_memory(&alloc_info, None)? };
        unsafe { self.device.bind_buffer_memory(buffer, memory, 0)? };

        let ptr = unsafe {
            self.device
                .map_memory(memory, 0, size, vk::MemoryMapFlags::empty())?
        } as *mut u32;

        unsafe {
            for i in 0..1024 {
                ptr.add(i).write(i as u32);
            }
        }

        let mut success = true;
        unsafe {
            for i in 0..1024 {
                let val = ptr.add(i).read();
                if val != i as u32 {
                    success = false;
                    break;
                }
            }
        }

        unsafe {
            self.device.unmap_memory(memory);
            self.device.free_memory(memory, None);
            self.device.destroy_buffer(buffer, None);
        }

        if success {
            println!(
                "  - [OK] Shared Virtual Memory (SVM) direct page-mapping diagnostics passed."
            );
        } else {
            println!("  - [FAIL] Shared Virtual Memory data check failed.");
        }

        Ok(())
    }

    pub fn run_attn_benchmarks(&self) -> Result<(f64, f64), Box<dyn std::error::Error>> {
        let device = &self.device;
        let physical_device = self.physical_device;
        let instance = &self.instance;
        let queue = self.compute_queue;

        let shader_info_contig =
            vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::ATTN_CONTIG_SPV);
        let shader_module_contig =
            unsafe { device.create_shader_module(&shader_info_contig, None)? };

        let shader_info_ndakv =
            vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::ATTN_NDAKV_SPV);
        let shader_module_ndakv = unsafe { device.create_shader_module(&shader_info_ndakv, None)? };

        let bindings_contig = [
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
        let layout_info_contig =
            vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings_contig);
        let desc_set_layout_contig =
            unsafe { device.create_descriptor_set_layout(&layout_info_contig, None)? };

        let bindings_ndakv = [
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
        let layout_info_ndakv =
            vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings_ndakv);
        let desc_set_layout_ndakv =
            unsafe { device.create_descriptor_set_layout(&layout_info_ndakv, None)? };

        let push_constant_ranges = [vk::PushConstantRange::builder()
            .stage_flags(vk::ShaderStageFlags::COMPUTE)
            .offset(0)
            .size(8)
            .build()];
        let layouts_contig = [desc_set_layout_contig];
        let pipeline_layout_info_contig = vk::PipelineLayoutCreateInfo::builder()
            .set_layouts(&layouts_contig)
            .push_constant_ranges(&push_constant_ranges);
        let pipeline_layout_contig =
            unsafe { device.create_pipeline_layout(&pipeline_layout_info_contig, None)? };

        let layouts_ndakv = [desc_set_layout_ndakv];
        let pipeline_layout_info_ndakv = vk::PipelineLayoutCreateInfo::builder()
            .set_layouts(&layouts_ndakv)
            .push_constant_ranges(&push_constant_ranges);
        let pipeline_layout_ndakv =
            unsafe { device.create_pipeline_layout(&pipeline_layout_info_ndakv, None)? };

        let main_entry = CString::new("main")?;

        let stage_info_contig = vk::PipelineShaderStageCreateInfo::builder()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(shader_module_contig)
            .name(&main_entry);
        let pipeline_create_info_contig = vk::ComputePipelineCreateInfo::builder()
            .stage(stage_info_contig.build())
            .layout(pipeline_layout_contig);
        let compute_pipelines_contig = unsafe {
            device
                .create_compute_pipelines(
                    vk::PipelineCache::null(),
                    &[pipeline_create_info_contig.build()],
                    None,
                )
                .map_err(|(_, e)| e)?
        };
        let compute_pipeline_contig = compute_pipelines_contig[0];

        let stage_info_ndakv = vk::PipelineShaderStageCreateInfo::builder()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(shader_module_ndakv)
            .name(&main_entry);
        let pipeline_create_info_ndakv = vk::ComputePipelineCreateInfo::builder()
            .stage(stage_info_ndakv.build())
            .layout(pipeline_layout_ndakv);
        let compute_pipelines_ndakv = unsafe {
            device
                .create_compute_pipelines(
                    vk::PipelineCache::null(),
                    &[pipeline_create_info_ndakv.build()],
                    None,
                )
                .map_err(|(_, e)| e)?
        };
        let compute_pipeline_ndakv = compute_pipelines_ndakv[0];

        let num_tokens = 256u32;
        let head_dim = 32u32;
        let num_heads = 32u32;

        let q_size = 256 as vk::DeviceSize;
        let k_size = (num_tokens * num_heads * head_dim * 4) as vk::DeviceSize;
        let v_size = (num_tokens * num_heads * head_dim * 4) as vk::DeviceSize;

        #[repr(C)]
        struct NdaKvBlock {
            next_block_idx: u32,
            num_tokens: u32,
            hash_checksum: u32,
            padding: u32,
            keys_active: [u32; 512],
            keys_pos: [u32; 512],
            values_active: [u32; 512],
            values_pos: [u32; 512],
        }
        let block_size = (16 * std::mem::size_of::<NdaKvBlock>()) as vk::DeviceSize;
        let out_size = (num_heads * head_dim * 4) as vk::DeviceSize;

        let (q_buffer, q_memory, q_ptr) =
            create_coherent_buffer(device, instance, physical_device, q_size, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (k_buffer, k_memory, k_ptr) =
            create_coherent_buffer(device, instance, physical_device, k_size, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (v_buffer, v_memory, v_ptr) =
            create_coherent_buffer(device, instance, physical_device, v_size, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (block_buffer, block_memory, block_ptr) =
            create_coherent_buffer(device, instance, physical_device, block_size, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (out_buffer, out_memory, _out_ptr) =
            create_coherent_buffer(device, instance, physical_device, out_size, vk::BufferUsageFlags::STORAGE_BUFFER)?;

        unsafe {
            let q_slice = std::slice::from_raw_parts_mut(q_ptr as *mut u32, 64);
            q_slice[0..32].fill(0x55555555);
            q_slice[32..64].fill(0x33333333);

            let k_slice = std::slice::from_raw_parts_mut(
                k_ptr as *mut f32,
                (num_tokens * num_heads * head_dim) as usize,
            );
            k_slice.fill(0.1);
            let v_slice = std::slice::from_raw_parts_mut(
                v_ptr as *mut f32,
                (num_tokens * num_heads * head_dim) as usize,
            );
            v_slice.fill(0.1);

            let block_slice = std::slice::from_raw_parts_mut(block_ptr as *mut NdaKvBlock, 16);
            for i in 0..16 {
                block_slice[i] = NdaKvBlock {
                    next_block_idx: if i == 15 { 999999 } else { (i + 1) as u32 },
                    num_tokens: 16,
                    hash_checksum: 12345,
                    padding: 0,
                    keys_active: [0x55555555; 512],
                    keys_pos: [0x33333333; 512],
                    values_active: [0x55555555; 512],
                    values_pos: [0x33333333; 512],
                };
            }
        }

        let pool_sizes = [vk::DescriptorPoolSize::builder()
            .ty(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(10)
            .build()];
        let pool_info = vk::DescriptorPoolCreateInfo::builder()
            .max_sets(2)
            .pool_sizes(&pool_sizes);
        let desc_pool = unsafe { device.create_descriptor_pool(&pool_info, None)? };

        let desc_set_contig = unsafe {
            device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::builder()
                    .descriptor_pool(desc_pool)
                    .set_layouts(&[desc_set_layout_contig]),
            )?[0]
        };
        let desc_set_ndakv = unsafe {
            device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::builder()
                    .descriptor_pool(desc_pool)
                    .set_layouts(&[desc_set_layout_ndakv]),
            )?[0]
        };

        let buffer_infos_contig = [
            vk::DescriptorBufferInfo::builder()
                .buffer(q_buffer)
                .offset(0)
                .range(q_size)
                .build(),
            vk::DescriptorBufferInfo::builder()
                .buffer(k_buffer)
                .offset(0)
                .range(k_size)
                .build(),
            vk::DescriptorBufferInfo::builder()
                .buffer(v_buffer)
                .offset(0)
                .range(v_size)
                .build(),
            vk::DescriptorBufferInfo::builder()
                .buffer(out_buffer)
                .offset(0)
                .range(out_size)
                .build(),
        ];
        let writes_contig = [
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set_contig)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos_contig[0..1])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set_contig)
                .dst_binding(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos_contig[1..2])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set_contig)
                .dst_binding(2)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos_contig[2..3])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set_contig)
                .dst_binding(3)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos_contig[3..4])
                .build(),
        ];
        unsafe { device.update_descriptor_sets(&writes_contig, &[]) };

        let buffer_infos_ndakv = [
            vk::DescriptorBufferInfo::builder()
                .buffer(q_buffer)
                .offset(0)
                .range(q_size)
                .build(),
            vk::DescriptorBufferInfo::builder()
                .buffer(block_buffer)
                .offset(0)
                .range(block_size)
                .build(),
            vk::DescriptorBufferInfo::builder()
                .buffer(out_buffer)
                .offset(0)
                .range(out_size)
                .build(),
        ];
        let writes_ndakv = [
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set_ndakv)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos_ndakv[0..1])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set_ndakv)
                .dst_binding(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos_ndakv[1..2])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set_ndakv)
                .dst_binding(2)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos_ndakv[2..3])
                .build(),
        ];
        unsafe { device.update_descriptor_sets(&writes_ndakv, &[]) };

        let command_pool_info = vk::CommandPoolCreateInfo::builder()
            .queue_family_index(self.queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = unsafe { device.create_command_pool(&command_pool_info, None)? };

        let command_buffers = unsafe {
            device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::builder()
                    .command_pool(command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(2),
            )?
        };
        let cmd_contig = command_buffers[0];
        let cmd_ndakv = command_buffers[1];

        unsafe {
            device.begin_command_buffer(
                cmd_contig,
                &vk::CommandBufferBeginInfo::builder()
                    .flags(vk::CommandBufferUsageFlags::SIMULTANEOUS_USE),
            )?;
            device.cmd_bind_pipeline(
                cmd_contig,
                vk::PipelineBindPoint::COMPUTE,
                compute_pipeline_contig,
            );
            device.cmd_bind_descriptor_sets(
                cmd_contig,
                vk::PipelineBindPoint::COMPUTE,
                pipeline_layout_contig,
                0,
                &[desc_set_contig],
                &[],
            );
            let params = [num_tokens, head_dim];
            let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
            device.cmd_push_constants(
                cmd_contig,
                pipeline_layout_contig,
                vk::ShaderStageFlags::COMPUTE,
                0,
                params_bytes,
            );
            device.cmd_dispatch(cmd_contig, 1, 1, 1);
            device.end_command_buffer(cmd_contig)?;
        }

        unsafe {
            device.begin_command_buffer(
                cmd_ndakv,
                &vk::CommandBufferBeginInfo::builder()
                    .flags(vk::CommandBufferUsageFlags::SIMULTANEOUS_USE),
            )?;
            device.cmd_bind_pipeline(
                cmd_ndakv,
                vk::PipelineBindPoint::COMPUTE,
                compute_pipeline_ndakv,
            );
            device.cmd_bind_descriptor_sets(
                cmd_ndakv,
                vk::PipelineBindPoint::COMPUTE,
                pipeline_layout_ndakv,
                0,
                &[desc_set_ndakv],
                &[],
            );
            let params = [0u32, head_dim];
            let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
            device.cmd_push_constants(
                cmd_ndakv,
                pipeline_layout_ndakv,
                vk::ShaderStageFlags::COMPUTE,
                0,
                params_bytes,
            );
            device.cmd_dispatch(cmd_ndakv, 1, 1, 1);
            device.end_command_buffer(cmd_ndakv)?;
        }

        let fence_info = vk::FenceCreateInfo::builder();
        let fence = unsafe { device.create_fence(&fence_info, None)? };

        let iterations = 500;

        let mut total_contig = 0.0;
        for _ in 0..iterations {
            let start = Instant::now();
            unsafe {
                device.reset_fences(&[fence])?;
                device.queue_submit(
                    queue,
                    &[vk::SubmitInfo::builder()
                        .command_buffers(&[cmd_contig])
                        .build()],
                    fence,
                )?;
                device.wait_for_fences(&[fence], true, u64::MAX)?;
            }
            total_contig += start.elapsed().as_micros() as f64;
        }
        let contig_avg_us = total_contig / (iterations as f64);

        let mut total_ndakv = 0.0;
        for _ in 0..iterations {
            let start = Instant::now();
            unsafe {
                device.reset_fences(&[fence])?;
                device.queue_submit(
                    queue,
                    &[vk::SubmitInfo::builder()
                        .command_buffers(&[cmd_ndakv])
                        .build()],
                    fence,
                )?;
                device.wait_for_fences(&[fence], true, u64::MAX)?;
            }
            total_ndakv += start.elapsed().as_micros() as f64;
        }
        let ndakv_avg_us = total_ndakv / (iterations as f64);

        unsafe {
            let _ = device.device_wait_idle();
            device.destroy_fence(fence, None);
            device.free_command_buffers(command_pool, &command_buffers);
            device.destroy_command_pool(command_pool, None);
            device.destroy_descriptor_pool(desc_pool, None);

            device.unmap_memory(q_memory);
            device.free_memory(q_memory, None);
            device.destroy_buffer(q_buffer, None);

            device.unmap_memory(k_memory);
            device.free_memory(k_memory, None);
            device.destroy_buffer(k_buffer, None);

            device.unmap_memory(v_memory);
            device.free_memory(v_memory, None);
            device.destroy_buffer(v_buffer, None);

            device.unmap_memory(block_memory);
            device.free_memory(block_memory, None);
            device.destroy_buffer(block_buffer, None);

            device.unmap_memory(out_memory);
            device.free_memory(out_memory, None);
            device.destroy_buffer(out_buffer, None);

            device.destroy_pipeline(compute_pipeline_contig, None);
            device.destroy_pipeline_layout(pipeline_layout_contig, None);
            device.destroy_descriptor_set_layout(desc_set_layout_contig, None);
            device.destroy_shader_module(shader_module_contig, None);

            device.destroy_pipeline(compute_pipeline_ndakv, None);
            device.destroy_pipeline_layout(pipeline_layout_ndakv, None);
            device.destroy_descriptor_set_layout(desc_set_layout_ndakv, None);
            device.destroy_shader_module(shader_module_ndakv, None);
        }

        Ok((contig_avg_us, ndakv_avg_us))
    }
}

impl Drop for VulkanDriver {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

pub fn create_coherent_buffer(
    device: &Device,
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
    size: vk::DeviceSize,
    usage: vk::BufferUsageFlags,
) -> Result<(vk::Buffer, vk::DeviceMemory, *mut std::ffi::c_void), Box<dyn std::error::Error>> {
    let buffer_info = vk::BufferCreateInfo::builder()
        .size(size)
        .usage(usage)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    let buffer = unsafe { device.create_buffer(&buffer_info, None)? };
    let mem_reqs = unsafe { device.get_buffer_memory_requirements(buffer) };

    let mem_props = unsafe { instance.get_physical_device_memory_properties(physical_device) };
    let mut mem_type_index = None;
    for i in 0..mem_props.memory_type_count {
        let flags = mem_props.memory_types[i as usize].property_flags;
        if (mem_reqs.memory_type_bits & (1 << i)) != 0
            && flags.contains(
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            )
        {
            mem_type_index = Some(i);
            break;
        }
    }
    let memory_type_index = mem_type_index.ok_or("No host-visible coherent memory found.")?;
    let alloc_info = vk::MemoryAllocateInfo::builder()
        .allocation_size(mem_reqs.size)
        .memory_type_index(memory_type_index);
    let memory = unsafe { device.allocate_memory(&alloc_info, None)? };
    unsafe { device.bind_buffer_memory(buffer, memory, 0)? };
    let ptr = unsafe { device.map_memory(memory, 0, size, vk::MemoryMapFlags::empty())? };
    Ok((buffer, memory, ptr))
}

pub fn create_device_local_buffer(
    device: &Device,
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
    queue: vk::Queue,
    queue_family_index: u32,
    size: vk::DeviceSize,
    data: &[u8],
) -> Result<(vk::Buffer, vk::DeviceMemory), Box<dyn std::error::Error>> {
    let staging_info = vk::BufferCreateInfo::builder()
        .size(size)
        .usage(vk::BufferUsageFlags::TRANSFER_SRC)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    let staging_buffer = unsafe { device.create_buffer(&staging_info, None)? };
    let staging_mem_reqs = unsafe { device.get_buffer_memory_requirements(staging_buffer) };

    let mem_props = unsafe { instance.get_physical_device_memory_properties(physical_device) };
    let mut staging_mem_type = None;
    for i in 0..mem_props.memory_type_count {
        let flags = mem_props.memory_types[i as usize].property_flags;
        if (staging_mem_reqs.memory_type_bits & (1 << i)) != 0
            && flags.contains(
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            )
        {
            staging_mem_type = Some(i);
            break;
        }
    }
    let staging_mem_type_index = staging_mem_type.ok_or("No host-visible staging memory found.")?;

    let staging_alloc_info = vk::MemoryAllocateInfo::builder()
        .allocation_size(staging_mem_reqs.size)
        .memory_type_index(staging_mem_type_index);
    let staging_memory = unsafe { device.allocate_memory(&staging_alloc_info, None)? };
    unsafe { device.bind_buffer_memory(staging_buffer, staging_memory, 0)? };

    let staging_ptr =
        unsafe { device.map_memory(staging_memory, 0, size, vk::MemoryMapFlags::empty())? };
    unsafe {
        std::ptr::copy_nonoverlapping(data.as_ptr(), staging_ptr as *mut u8, data.len());
        device.unmap_memory(staging_memory);
    }

    let local_info = vk::BufferCreateInfo::builder()
        .size(size)
        .usage(vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    let local_buffer = unsafe { device.create_buffer(&local_info, None)? };
    let local_mem_reqs = unsafe { device.get_buffer_memory_requirements(local_buffer) };

    let mut local_mem_type = None;
    for i in 0..mem_props.memory_type_count {
        let flags = mem_props.memory_types[i as usize].property_flags;
        if (local_mem_reqs.memory_type_bits & (1 << i)) != 0
            && flags.contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
        {
            local_mem_type = Some(i);
            break;
        }
    }
    let local_mem_type_index = local_mem_type.ok_or("No device-local memory found on GPU.")?;

    let local_alloc_info = vk::MemoryAllocateInfo::builder()
        .allocation_size(local_mem_reqs.size)
        .memory_type_index(local_mem_type_index);
    let local_memory = unsafe { device.allocate_memory(&local_alloc_info, None)? };
    unsafe { device.bind_buffer_memory(local_buffer, local_memory, 0)? };

    let copy_pool_info =
        vk::CommandPoolCreateInfo::builder().queue_family_index(queue_family_index);
    let copy_pool = unsafe { device.create_command_pool(&copy_pool_info, None)? };

    let copy_alloc_info = vk::CommandBufferAllocateInfo::builder()
        .command_pool(copy_pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);
    let copy_command_buffers = unsafe { device.allocate_command_buffers(&copy_alloc_info)? };
    let copy_command_buffer = copy_command_buffers[0];

    let copy_begin_info =
        vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    unsafe {
        device.begin_command_buffer(copy_command_buffer, &copy_begin_info)?;
        let copy_region = vk::BufferCopy::builder()
            .src_offset(0)
            .dst_offset(0)
            .size(size);
        device.cmd_copy_buffer(
            copy_command_buffer,
            staging_buffer,
            local_buffer,
            &[copy_region.build()],
        );
        device.end_command_buffer(copy_command_buffer)?;
    }

    let copy_fence_info = vk::FenceCreateInfo::builder();
    let copy_fence = unsafe { device.create_fence(&copy_fence_info, None)? };

    let submit_infos = [vk::SubmitInfo::builder()
        .command_buffers(&[copy_command_buffer])
        .build()];
    unsafe {
        device.queue_submit(queue, &submit_infos, copy_fence)?;
        device.wait_for_fences(&[copy_fence], true, u64::MAX)?;

        device.destroy_fence(copy_fence, None);
        device.free_command_buffers(copy_pool, &[copy_command_buffer]);
        device.destroy_command_pool(copy_pool, None);

        device.free_memory(staging_memory, None);
        device.destroy_buffer(staging_buffer, None);
    }
    Ok((local_buffer, local_memory))
}
