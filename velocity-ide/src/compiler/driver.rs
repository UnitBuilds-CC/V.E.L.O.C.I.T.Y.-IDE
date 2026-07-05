#![allow(dead_code)]
use std::ffi::CString;
use std::time::Instant;
use ash::vk::Handle;
use ash::{vk, Entry, Instance, Device};

pub struct VulkanDriver {
    pub entry: Entry,
    pub instance: Instance,
    pub physical_device: vk::PhysicalDevice,
    pub device: Device,
    pub queue_family_index: u32,
    pub compute_queue: vk::Queue,
    
    pub shared_input_buffer: vk::Buffer,
    pub shared_input_memory: vk::DeviceMemory,
    pub shared_input_ptr: *mut std::ffi::c_void,
}

impl VulkanDriver {
    pub fn init() -> Result<Self, Box<dyn std::error::Error>> {
        // Load the Vulkan library
        let entry = unsafe { Entry::load()? };

        // App metadata
        let app_name = CString::new("V.E.L.O.C.I.T.Y. IDE Engine")?;
        let engine_name = CString::new("V-NCE JIT Compiler")?;
        let app_info = vk::ApplicationInfo::builder()
            .application_name(&app_name)
            .application_version(vk::make_api_version(0, 1, 0, 0))
            .engine_name(&engine_name)
            .engine_version(vk::make_api_version(0, 1, 0, 0))
            .api_version(vk::API_VERSION_1_2);

        // Instance creation configurations
        let create_info = vk::InstanceCreateInfo::builder()
            .application_info(&app_info);

        let instance = unsafe { entry.create_instance(&create_info, None)? };

        // Enumerate physical devices (GPUs)
        let physical_devices = unsafe { instance.enumerate_physical_devices()? };
        if physical_devices.is_empty() {
            return Err("No Vulkan-compatible physical devices (GPUs) found.".into());
        }

        // Find the best device (prefer Discrete GPU)
        let mut selected_device = None;
        let mut selected_queue_family = None;
        let mut selected_device_name = String::new();

        eprintln!("Direct Vulkan GPU Enumeration:");
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
            eprintln!("  - [{}] {} (Type: {})", pd.as_raw(), name, type_str);

            // Look for a compute queue family
            let queue_properties = unsafe { instance.get_physical_device_queue_family_properties(pd) };
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

        eprintln!("Selected GPU: {}", selected_device_name);

        // Logical Device Creation
        let queue_priorities = [1.0f32];
        let queue_create_info = vk::DeviceQueueCreateInfo::builder()
            .queue_family_index(queue_family_index)
            .queue_priorities(&queue_priorities);

        let device_create_infos = [queue_create_info.build()];
        let device_create_info = vk::DeviceCreateInfo::builder()
            .queue_create_infos(&device_create_infos);

        let device = unsafe { instance.create_device(physical_device, &device_create_info, None)? };
        let compute_queue = unsafe { device.get_device_queue(queue_family_index, 0) };

        let shared_size = (65536 * 4) as vk::DeviceSize; // 256 KB (lots of room for future layers)
        let (shared_input_buffer, shared_input_memory, shared_input_ptr) = create_coherent_buffer(&device, &instance, physical_device, shared_size, vk::BufferUsageFlags::STORAGE_BUFFER)?;

        Ok(Self {
            entry,
            instance,
            physical_device,
            device,
            queue_family_index,
            compute_queue,
            shared_input_buffer,
            shared_input_memory,
            shared_input_ptr,
        })
    }

    pub fn run_diagnostics(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("V.E.L.O.C.I.T.Y. V-NCE Diagnostic Run:");
        println!("  - Vulkan logical device handles initialized.");
        println!("  - Compute Queue Family Index: {}", self.queue_family_index);
        println!("  - Compute queue successfully bounded to thread context.");
        
        // Diagnostic mapping test (Allocates a small buffer in host-visible GPU memory)
        let size = 1024 * vk::DeviceSize::from(4u32); // 4KB
        let buffer_info = vk::BufferCreateInfo::builder()
            .size(size)
            .usage(vk::BufferUsageFlags::STORAGE_BUFFER)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);

        let buffer = unsafe { self.device.create_buffer(&buffer_info, None)? };
        let mem_reqs = unsafe { self.device.get_buffer_memory_requirements(buffer) };

        // Find memory type index for host-visible memory
        let mem_props = unsafe { self.instance.get_physical_device_memory_properties(self.physical_device) };
        let mut mem_type_index = None;
        for i in 0..mem_props.memory_type_count {
            let flags = mem_props.memory_types[i as usize].property_flags;
            if (mem_reqs.memory_type_bits & (1 << i)) != 0
                && flags.contains(vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT)
            {
                mem_type_index = Some(i);
                break;
            }
        }

        let memory_type_index = mem_type_index.ok_or("No host-visible coherent memory found on GPU.")?;
        
        let alloc_info = vk::MemoryAllocateInfo::builder()
            .allocation_size(mem_reqs.size)
            .memory_type_index(memory_type_index);

        let memory = unsafe { self.device.allocate_memory(&alloc_info, None)? };
        unsafe { self.device.bind_buffer_memory(buffer, memory, 0)? };

        // Direct Memory Map (SVM Pipeline)
        let ptr = unsafe { self.device.map_memory(memory, 0, size, vk::MemoryMapFlags::empty())? } as *mut u32;
        
        // Write test data natively from host CPU
        unsafe {
            for i in 0..1024 {
                ptr.add(i).write(i as u32);
            }
        }

        // Verify read back
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

        // Unmap & cleanup
        unsafe {
            self.device.unmap_memory(memory);
            self.device.free_memory(memory, None);
            self.device.destroy_buffer(buffer, None);
        }

        if success {
            println!("  - [OK] Shared Virtual Memory (SVM) direct page-mapping diagnostics passed.");
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

        // 1. Create Shaders
        let shader_info_contig = vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::ATTN_CONTIG_SPV);
        let shader_module_contig = unsafe { device.create_shader_module(&shader_info_contig, None)? };

        let shader_info_ndakv = vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::ATTN_NDAKV_SPV);
        let shader_module_ndakv = unsafe { device.create_shader_module(&shader_info_ndakv, None)? };

        // 2. Descriptor layouts
        let bindings_contig = [
            vk::DescriptorSetLayoutBinding::builder().binding(0).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
            vk::DescriptorSetLayoutBinding::builder().binding(1).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
            vk::DescriptorSetLayoutBinding::builder().binding(2).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
            vk::DescriptorSetLayoutBinding::builder().binding(3).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
        ];
        let layout_info_contig = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings_contig);
        let desc_set_layout_contig = unsafe { device.create_descriptor_set_layout(&layout_info_contig, None)? };

        let bindings_ndakv = [
            vk::DescriptorSetLayoutBinding::builder().binding(0).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
            vk::DescriptorSetLayoutBinding::builder().binding(1).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
            vk::DescriptorSetLayoutBinding::builder().binding(2).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
        ];
        let layout_info_ndakv = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings_ndakv);
        let desc_set_layout_ndakv = unsafe { device.create_descriptor_set_layout(&layout_info_ndakv, None)? };

        // 3. Pipeline layouts
        let push_constant_ranges = [
            vk::PushConstantRange::builder().stage_flags(vk::ShaderStageFlags::COMPUTE).offset(0).size(8).build(),
        ];
        let layouts_contig = [desc_set_layout_contig];
        let pipeline_layout_info_contig = vk::PipelineLayoutCreateInfo::builder().set_layouts(&layouts_contig).push_constant_ranges(&push_constant_ranges);
        let pipeline_layout_contig = unsafe { device.create_pipeline_layout(&pipeline_layout_info_contig, None)? };

        let layouts_ndakv = [desc_set_layout_ndakv];
        let pipeline_layout_info_ndakv = vk::PipelineLayoutCreateInfo::builder().set_layouts(&layouts_ndakv).push_constant_ranges(&push_constant_ranges);
        let pipeline_layout_ndakv = unsafe { device.create_pipeline_layout(&pipeline_layout_info_ndakv, None)? };

        // 4. Compute Pipelines
        let main_entry = CString::new("main")?;
        
        let stage_info_contig = vk::PipelineShaderStageCreateInfo::builder().stage(vk::ShaderStageFlags::COMPUTE).module(shader_module_contig).name(&main_entry);
        let pipeline_create_info_contig = vk::ComputePipelineCreateInfo::builder().stage(stage_info_contig.build()).layout(pipeline_layout_contig);
        let compute_pipelines_contig = unsafe {
            device.create_compute_pipelines(vk::PipelineCache::null(), &[pipeline_create_info_contig.build()], None).map_err(|(_, e)| e)?
        };
        let compute_pipeline_contig = compute_pipelines_contig[0];

        let stage_info_ndakv = vk::PipelineShaderStageCreateInfo::builder().stage(vk::ShaderStageFlags::COMPUTE).module(shader_module_ndakv).name(&main_entry);
        let pipeline_create_info_ndakv = vk::ComputePipelineCreateInfo::builder().stage(stage_info_ndakv.build()).layout(pipeline_layout_ndakv);
        let compute_pipelines_ndakv = unsafe {
            device.create_compute_pipelines(vk::PipelineCache::null(), &[pipeline_create_info_ndakv.build()], None).map_err(|(_, e)| e)?
        };
        let compute_pipeline_ndakv = compute_pipelines_ndakv[0];

        // 5. Create buffers helper
        let create_coherent_buffer = |size: vk::DeviceSize, usage: vk::BufferUsageFlags| -> Result<(vk::Buffer, vk::DeviceMemory, *mut std::ffi::c_void), Box<dyn std::error::Error>> {
            let buffer_info = vk::BufferCreateInfo::builder().size(size).usage(usage).sharing_mode(vk::SharingMode::EXCLUSIVE);
            let buffer = unsafe { device.create_buffer(&buffer_info, None)? };
            let mem_reqs = unsafe { device.get_buffer_memory_requirements(buffer) };
            let mem_props = unsafe { instance.get_physical_device_memory_properties(physical_device) };
            let mut mem_type_index = None;
            for i in 0..mem_props.memory_type_count {
                let flags = mem_props.memory_types[i as usize].property_flags;
                if (mem_reqs.memory_type_bits & (1 << i)) != 0 && flags.contains(vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT) {
                    mem_type_index = Some(i);
                    break;
                }
            }
            let memory_type_index = mem_type_index.ok_or("No host-visible coherent memory found.")?;
            let alloc_info = vk::MemoryAllocateInfo::builder().allocation_size(mem_reqs.size).memory_type_index(memory_type_index);
            let memory = unsafe { device.allocate_memory(&alloc_info, None)? };
            unsafe { device.bind_buffer_memory(buffer, memory, 0)? };
            let ptr = unsafe { device.map_memory(memory, 0, size, vk::MemoryMapFlags::empty())? };
            Ok((buffer, memory, ptr))
        };

        // Dimensions
        let num_tokens = 256u32;
        let head_dim = 32u32;
        let num_heads = 32u32;

        // Buffer sizes
        let q_size = 256 as vk::DeviceSize; // 32 words active + 32 words pos = 64 words = 256 bytes
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

        let (q_buffer, q_memory, q_ptr) = create_coherent_buffer(q_size, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (k_buffer, k_memory, k_ptr) = create_coherent_buffer(k_size, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (v_buffer, v_memory, v_ptr) = create_coherent_buffer(v_size, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (block_buffer, block_memory, block_ptr) = create_coherent_buffer(block_size, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (out_buffer, out_memory, _out_ptr) = create_coherent_buffer(out_size, vk::BufferUsageFlags::STORAGE_BUFFER)?;

        // Populate Q, K, V
        unsafe {
            let q_slice = std::slice::from_raw_parts_mut(q_ptr as *mut u32, 64);
            q_slice[0..32].fill(0x55555555); // active
            q_slice[32..64].fill(0x33333333); // pos
            
            let k_slice = std::slice::from_raw_parts_mut(k_ptr as *mut f32, (num_tokens * num_heads * head_dim) as usize);
            k_slice.fill(0.1);
            let v_slice = std::slice::from_raw_parts_mut(v_ptr as *mut f32, (num_tokens * num_heads * head_dim) as usize);
            v_slice.fill(0.1);

            // Populate Blocks
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

        // 6. Descriptor Pool
        let pool_sizes = [vk::DescriptorPoolSize::builder().ty(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(10).build()];
        let pool_info = vk::DescriptorPoolCreateInfo::builder().max_sets(2).pool_sizes(&pool_sizes);
        let desc_pool = unsafe { device.create_descriptor_pool(&pool_info, None)? };

        // Allocate sets
        let desc_set_contig = unsafe { device.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::builder().descriptor_pool(desc_pool).set_layouts(&[desc_set_layout_contig]))?[0] };
        let desc_set_ndakv = unsafe { device.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::builder().descriptor_pool(desc_pool).set_layouts(&[desc_set_layout_ndakv]))?[0] };

        // Bind sets
        let buffer_infos_contig = [
            vk::DescriptorBufferInfo::builder().buffer(q_buffer).offset(0).range(q_size).build(),
            vk::DescriptorBufferInfo::builder().buffer(k_buffer).offset(0).range(k_size).build(),
            vk::DescriptorBufferInfo::builder().buffer(v_buffer).offset(0).range(v_size).build(),
            vk::DescriptorBufferInfo::builder().buffer(out_buffer).offset(0).range(out_size).build(),
        ];
        let writes_contig = [
            vk::WriteDescriptorSet::builder().dst_set(desc_set_contig).dst_binding(0).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&buffer_infos_contig[0..1]).build(),
            vk::WriteDescriptorSet::builder().dst_set(desc_set_contig).dst_binding(1).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&buffer_infos_contig[1..2]).build(),
            vk::WriteDescriptorSet::builder().dst_set(desc_set_contig).dst_binding(2).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&buffer_infos_contig[2..3]).build(),
            vk::WriteDescriptorSet::builder().dst_set(desc_set_contig).dst_binding(3).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&buffer_infos_contig[3..4]).build(),
        ];
        unsafe { device.update_descriptor_sets(&writes_contig, &[]) };

        let buffer_infos_ndakv = [
            vk::DescriptorBufferInfo::builder().buffer(q_buffer).offset(0).range(q_size).build(),
            vk::DescriptorBufferInfo::builder().buffer(block_buffer).offset(0).range(block_size).build(),
            vk::DescriptorBufferInfo::builder().buffer(out_buffer).offset(0).range(out_size).build(),
        ];
        let writes_ndakv = [
            vk::WriteDescriptorSet::builder().dst_set(desc_set_ndakv).dst_binding(0).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&buffer_infos_ndakv[0..1]).build(),
            vk::WriteDescriptorSet::builder().dst_set(desc_set_ndakv).dst_binding(1).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&buffer_infos_ndakv[1..2]).build(),
            vk::WriteDescriptorSet::builder().dst_set(desc_set_ndakv).dst_binding(2).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&buffer_infos_ndakv[2..3]).build(),
        ];
        unsafe { device.update_descriptor_sets(&writes_ndakv, &[]) };

        // 7. Command Pool & Command Buffers
        let command_pool_info = vk::CommandPoolCreateInfo::builder().queue_family_index(self.queue_family_index).flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = unsafe { device.create_command_pool(&command_pool_info, None)? };

        let command_buffers = unsafe { device.allocate_command_buffers(&vk::CommandBufferAllocateInfo::builder().command_pool(command_pool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(2))? };
        let cmd_contig = command_buffers[0];
        let cmd_ndakv = command_buffers[1];

        // Record contiguous cmd
        unsafe {
            device.begin_command_buffer(cmd_contig, &vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::SIMULTANEOUS_USE))?;
            device.cmd_bind_pipeline(cmd_contig, vk::PipelineBindPoint::COMPUTE, compute_pipeline_contig);
            device.cmd_bind_descriptor_sets(cmd_contig, vk::PipelineBindPoint::COMPUTE, pipeline_layout_contig, 0, &[desc_set_contig], &[]);
            let params = [num_tokens, head_dim];
            let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
            device.cmd_push_constants(cmd_contig, pipeline_layout_contig, vk::ShaderStageFlags::COMPUTE, 0, params_bytes);
            device.cmd_dispatch(cmd_contig, 1, 1, 1);
            device.end_command_buffer(cmd_contig)?;
        }

        // Record ndakv cmd
        unsafe {
            device.begin_command_buffer(cmd_ndakv, &vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::SIMULTANEOUS_USE))?;
            device.cmd_bind_pipeline(cmd_ndakv, vk::PipelineBindPoint::COMPUTE, compute_pipeline_ndakv);
            device.cmd_bind_descriptor_sets(cmd_ndakv, vk::PipelineBindPoint::COMPUTE, pipeline_layout_ndakv, 0, &[desc_set_ndakv], &[]);
            let params = [0u32, head_dim];
            let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
            device.cmd_push_constants(cmd_ndakv, pipeline_layout_ndakv, vk::ShaderStageFlags::COMPUTE, 0, params_bytes);
            device.cmd_dispatch(cmd_ndakv, 1, 1, 1);
            device.end_command_buffer(cmd_ndakv)?;
        }

        // Fences
        let fence_info = vk::FenceCreateInfo::builder();
        let fence = unsafe { device.create_fence(&fence_info, None)? };

        // 8. Run Benchmarks
        let iterations = 500;

        // Contiguous
        let mut total_contig = 0.0;
        for _ in 0..iterations {
            let start = Instant::now();
            unsafe {
                device.reset_fences(&[fence])?;
                device.queue_submit(queue, &[vk::SubmitInfo::builder().command_buffers(&[cmd_contig]).build()], fence)?;
                device.wait_for_fences(&[fence], true, u64::MAX)?;
            }
            total_contig += start.elapsed().as_micros() as f64;
        }
        let contig_avg_us = total_contig / (iterations as f64);

        // NDA-KV
        let mut total_ndakv = 0.0;
        for _ in 0..iterations {
            let start = Instant::now();
            unsafe {
                device.reset_fences(&[fence])?;
                device.queue_submit(queue, &[vk::SubmitInfo::builder().command_buffers(&[cmd_ndakv]).build()], fence)?;
                device.wait_for_fences(&[fence], true, u64::MAX)?;
            }
            total_ndakv += start.elapsed().as_micros() as f64;
        }
        let ndakv_avg_us = total_ndakv / (iterations as f64);

        // Cleanup
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

fn create_coherent_buffer(
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
            && flags.contains(vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT)
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

fn create_device_local_buffer(
    device: &Device,
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
    queue: vk::Queue,
    queue_family_index: u32,
    size: vk::DeviceSize,
    data: &[u8],
) -> Result<(vk::Buffer, vk::DeviceMemory), Box<dyn std::error::Error>> {
    // 1. Create staging buffer
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
            && flags.contains(vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT)
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
    
    // Copy data to staging
    let staging_ptr = unsafe { device.map_memory(staging_memory, 0, size, vk::MemoryMapFlags::empty())? };
    unsafe {
        std::ptr::copy_nonoverlapping(data.as_ptr(), staging_ptr as *mut u8, data.len());
        device.unmap_memory(staging_memory);
    }
    
    // 2. Create device local buffer
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
    
    // 3. Copy via command buffer
    let copy_pool_info = vk::CommandPoolCreateInfo::builder()
        .queue_family_index(queue_family_index);
    let copy_pool = unsafe { device.create_command_pool(&copy_pool_info, None)? };
    
    let copy_alloc_info = vk::CommandBufferAllocateInfo::builder()
        .command_pool(copy_pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);
    let copy_command_buffers = unsafe { device.allocate_command_buffers(&copy_alloc_info)? };
    let copy_command_buffer = copy_command_buffers[0];
    
    let copy_begin_info = vk::CommandBufferBeginInfo::builder()
        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    unsafe {
        device.begin_command_buffer(copy_command_buffer, &copy_begin_info)?;
        let copy_region = vk::BufferCopy::builder()
            .src_offset(0)
            .dst_offset(0)
            .size(size);
        device.cmd_copy_buffer(copy_command_buffer, staging_buffer, local_buffer, &[copy_region.build()]);
        device.end_command_buffer(copy_command_buffer)?;
    }
    
    let copy_fence_info = vk::FenceCreateInfo::builder();
    let copy_fence = unsafe { device.create_fence(&copy_fence_info, None)? };
    
    let submit_infos = [
        vk::SubmitInfo::builder()
            .command_buffers(&[copy_command_buffer])
            .build()
    ];
    unsafe {
        device.queue_submit(queue, &submit_infos, copy_fence)?;
        device.wait_for_fences(&[copy_fence], true, u64::MAX)?;
        
        // Cleanup staging resources and copy resources
        device.destroy_fence(copy_fence, None);
        device.free_command_buffers(copy_pool, &[copy_command_buffer]);
        device.destroy_command_pool(copy_pool, None);
        
        device.free_memory(staging_memory, None);
        device.destroy_buffer(staging_buffer, None);
    }
    Ok((local_buffer, local_memory))
}

fn pack_weights_uvec4(src: &[u8], k: usize, n: usize) -> Vec<u8> {
    let src_u32 = unsafe {
        std::slice::from_raw_parts(src.as_ptr() as *const u32, src.len() / 4)
    };
    let num_col_groups = k / 16;
    let num_col_groups_4 = num_col_groups / 4;
    let mut dest = vec![0u32; num_col_groups * n];
    
    for cg4 in 0..num_col_groups_4 {
        for row in 0..n {
            for offset in 0..4 {
                let cg = cg4 * 4 + offset;
                let src_idx = cg * n + row;
                let dest_idx = cg4 * n * 4 + row * 4 + offset;
                dest[dest_idx] = src_u32[src_idx];
            }
        }
    }
    
    unsafe {
        let bytes_ptr = dest.as_ptr() as *const u8;
        std::slice::from_raw_parts(bytes_ptr, dest.len() * 4).to_vec()
    }
}

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

        // 1. Create Shader Module
        let spv_code = if is_ternary {
            crate::compiler::shaders::TERNARY_SPV
        } else {
            crate::compiler::shaders::INT4_SPV
        };
        let shader_info = vk::ShaderModuleCreateInfo::builder().code(spv_code);
        let shader_module = unsafe { device.create_shader_module(&shader_info, None)? };

        // 2. Create Descriptor Set Layout
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

        // 3. Create Pipeline Layout with Push Constants
        let push_constant_ranges = [
            vk::PushConstantRange::builder()
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .offset(0)
                .size(8) // Two u32 (K, N)
                .build(),
        ];
        let layouts = [desc_set_layout];
        let pipeline_layout_info = vk::PipelineLayoutCreateInfo::builder()
            .set_layouts(&layouts)
            .push_constant_ranges(&push_constant_ranges);
        let pipeline_layout = unsafe { device.create_pipeline_layout(&pipeline_layout_info, None)? };

        // 4. Create Compute Pipeline
        let main_entry = CString::new("main")?;
        let stage_info = vk::PipelineShaderStageCreateInfo::builder()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(shader_module)
            .name(&main_entry);
        let pipeline_create_info = vk::ComputePipelineCreateInfo::builder()
            .stage(stage_info.build())
            .layout(pipeline_layout);
        let compute_pipelines = unsafe {
            device.create_compute_pipelines(vk::PipelineCache::null(), &[pipeline_create_info.build()], None)
                .map_err(|(_, e)| e)?
        };
        let compute_pipeline = compute_pipelines[0];

        // 5. Create Storage Buffers
        let input_size = if is_ternary {
            ((k / 16) * 4) as vk::DeviceSize
        } else {
            (k * 4) as vk::DeviceSize // f32 input
        };
        let weight_size = weight_bytes.len() as vk::DeviceSize;
        let output_size = (n * 4) as vk::DeviceSize;

        let (input_buffer, input_memory, input_ptr) = create_coherent_buffer(&device, &instance, physical_device, input_size, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (weight_buffer, weight_memory) = if is_ternary {
            let packed = pack_weights_uvec4(weight_bytes, k as usize, n as usize);
            create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, weight_size, &packed)?
        } else {
            create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, weight_size, weight_bytes)?
        };
        let (output_buffer, output_memory, output_ptr) = create_coherent_buffer(&device, &instance, physical_device, output_size, vk::BufferUsageFlags::STORAGE_BUFFER)?;

        // 6. Create Descriptor Pool & Allocate Descriptor Set
        let pool_sizes = [
            vk::DescriptorPoolSize::builder()
                .ty(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(3)
                .build(),
        ];
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

        // Bind storage buffers to descriptor set
        let buffer_infos = [
            vk::DescriptorBufferInfo::builder().buffer(input_buffer).offset(0).range(input_size).build(),
            vk::DescriptorBufferInfo::builder().buffer(weight_buffer).offset(0).range(weight_size).build(),
            vk::DescriptorBufferInfo::builder().buffer(output_buffer).offset(0).range(output_size).build(),
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

        // 7. Create Command Pool & Record Dispatch Commands
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
            device.cmd_bind_pipeline(command_buffer, vk::PipelineBindPoint::COMPUTE, compute_pipeline);
            device.cmd_bind_descriptor_sets(
                command_buffer,
                vk::PipelineBindPoint::COMPUTE,
                pipeline_layout,
                0,
                &[desc_set],
                &[],
            );
            
            // Push Constants (K, N)
            let params = [k, n];
            let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
            device.cmd_push_constants(
                command_buffer,
                pipeline_layout,
                vk::ShaderStageFlags::COMPUTE,
                0,
                params_bytes,
            );
            
            // Dispatch
            let workgroup_count = if is_ternary {
                (n + 255) / 256
            } else {
                (n + 63) / 64
            };
            device.cmd_dispatch(command_buffer, workgroup_count, 1, 1);
            device.end_command_buffer(command_buffer)?;
        }

        // 8. Create Fence
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

    pub fn run(&self, input_bytes: &[u8], output_floats: &mut [f32]) -> Result<f64, Box<dyn std::error::Error>> {
        // Stream input data
        unsafe {
            std::ptr::copy_nonoverlapping(input_bytes.as_ptr(), self.input_ptr as *mut u8, input_bytes.len());
        }

        // Submit
        let start = Instant::now();
        let command_buffers = [self.command_buffer];
        let submit_info = vk::SubmitInfo::builder()
            .command_buffers(&command_buffers);
        unsafe {
            self.device.reset_fences(&[self.fence])?;
            self.device.queue_submit(self.queue, &[submit_info.build()], self.fence)?;
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        let duration_us = start.elapsed().as_micros() as f64;

        // Stream output data
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
            self.device.destroy_pipeline_layout(self.pipeline_layout, None);
            self.device.destroy_descriptor_set_layout(self.desc_set_layout, None);
            self.device.destroy_shader_module(self.shader_module, None);
        }
    }
}


impl Drop for VulkanDriver {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_buffer(self.shared_input_buffer, None);
            self.device.free_memory(self.shared_input_memory, None);
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

pub struct VulkanQwenLayer {
    pub device: Device,
    pub queue: vk::Queue,
    
    // Shader Modules
    pub shader_int4: vk::ShaderModule,
    pub shader_act: vk::ShaderModule,
    
    // Descriptor Set Layout
    pub desc_set_layout: vk::DescriptorSetLayout,
    pub pipeline_layout: vk::PipelineLayout,
    
    // Pipelines
    pub pipeline_int4: vk::Pipeline,
    pub pipeline_act: vk::Pipeline,
    
    // Buffers & Memory
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
    
    // Weights
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
    
    // Descriptors
    pub desc_pool: vk::DescriptorPool,
    pub desc_sets: Vec<vk::DescriptorSet>,
    
    // Execution
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

        // 1. Create Shaders
        let shader_info_int4 = vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::INT4_SPV);
        let shader_int4 = unsafe { device.create_shader_module(&shader_info_int4, None)? };
        
        let shader_info_act = vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::ACT_QWEN_SPV);
        let shader_act = unsafe { device.create_shader_module(&shader_info_act, None)? };

        // 2. Create Descriptor Layout (same layout for all)
        let bindings = [
            vk::DescriptorSetLayoutBinding::builder().binding(0).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
            vk::DescriptorSetLayoutBinding::builder().binding(1).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
            vk::DescriptorSetLayoutBinding::builder().binding(2).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
        ];
        let layout_info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);
        let desc_set_layout = unsafe { device.create_descriptor_set_layout(&layout_info, None)? };

        // 3. Create Pipeline Layout
        let push_constant_ranges = [
            vk::PushConstantRange::builder().stage_flags(vk::ShaderStageFlags::COMPUTE).offset(0).size(8).build(),
        ];
        let layouts = [desc_set_layout];
        let pipeline_layout_info = vk::PipelineLayoutCreateInfo::builder().set_layouts(&layouts).push_constant_ranges(&push_constant_ranges);
        let pipeline_layout = unsafe { device.create_pipeline_layout(&pipeline_layout_info, None)? };

        // 4. Create Compute Pipelines
        let main_entry = CString::new("main")?;
        
        let stage_info_int4 = vk::PipelineShaderStageCreateInfo::builder().stage(vk::ShaderStageFlags::COMPUTE).module(shader_int4).name(&main_entry);
        let pipeline_create_info_int4 = vk::ComputePipelineCreateInfo::builder().stage(stage_info_int4.build()).layout(pipeline_layout);
        
        let stage_info_act = vk::PipelineShaderStageCreateInfo::builder().stage(vk::ShaderStageFlags::COMPUTE).module(shader_act).name(&main_entry);
        let pipeline_create_info_act = vk::ComputePipelineCreateInfo::builder().stage(stage_info_act.build()).layout(pipeline_layout);
        
        let pipelines = unsafe {
            device.create_compute_pipelines(vk::PipelineCache::null(), &[pipeline_create_info_int4.build(), pipeline_create_info_act.build()], None).map_err(|(_, e)| e)?
        };
        let pipeline_int4 = pipelines[0];
        let pipeline_act = pipelines[1];

        // 5. Create Buffers
        let (inputs_2304_buffer, inputs_2304_memory, inputs_2304_ptr) = create_coherent_buffer(&device, &instance, physical_device, 2304 * 4, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (out_2304_a_buffer, out_2304_a_memory, out_2304_a_ptr) = create_coherent_buffer(&device, &instance, physical_device, 2304 * 4, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        
        let (out_2304_b_buffer, out_2304_b_memory, _) = create_coherent_buffer(&device, &instance, physical_device, 2304 * 4, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (out_256_k_buffer, out_256_k_memory, _) = create_coherent_buffer(&device, &instance, physical_device, 256 * 4, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (out_256_v_buffer, out_256_v_memory, _) = create_coherent_buffer(&device, &instance, physical_device, 256 * 4, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (out_11008_gate_buffer, out_11008_gate_memory, _) = create_coherent_buffer(&device, &instance, physical_device, 11008 * 4, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (out_11008_up_buffer, out_11008_up_memory, _) = create_coherent_buffer(&device, &instance, physical_device, 11008 * 4, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (inputs_11008_buffer, inputs_11008_memory, _) = create_coherent_buffer(&device, &instance, physical_device, 11008 * 4, vk::BufferUsageFlags::STORAGE_BUFFER)?;

        // Weights
        let (weight_q_buffer, weight_q_memory) = create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, weight_q.len() as vk::DeviceSize, weight_q)?;
        let (weight_k_buffer, weight_k_memory) = create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, weight_k.len() as vk::DeviceSize, weight_k)?;
        let (weight_v_buffer, weight_v_memory) = create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, weight_v.len() as vk::DeviceSize, weight_v)?;
        let (weight_o_buffer, weight_o_memory) = create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, weight_o.len() as vk::DeviceSize, weight_o)?;
        let (weight_gate_buffer, weight_gate_memory) = create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, weight_gate.len() as vk::DeviceSize, weight_gate)?;
        let (weight_up_buffer, weight_up_memory) = create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, weight_up.len() as vk::DeviceSize, weight_up)?;
        let (weight_down_buffer, weight_down_memory) = create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, weight_down.len() as vk::DeviceSize, weight_down)?;

        // 6. Descriptor Pool
        let pool_sizes = [
            vk::DescriptorPoolSize::builder().ty(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(24).build(),
        ];
        let pool_info = vk::DescriptorPoolCreateInfo::builder().max_sets(8).pool_sizes(&pool_sizes);
        let desc_pool = unsafe { device.create_descriptor_pool(&pool_info, None)? };

        // Allocate 8 sets
        let layouts_vec = vec![desc_set_layout; 8];
        let alloc_info = vk::DescriptorSetAllocateInfo::builder().descriptor_pool(desc_pool).set_layouts(&layouts_vec);
        let desc_sets = unsafe { device.allocate_descriptor_sets(&alloc_info)? };
        
        let set_q = desc_sets[0];
        let set_k = desc_sets[1];
        let set_v = desc_sets[2];
        let set_o = desc_sets[3];
        let set_gate = desc_sets[4];
        let set_up = desc_sets[5];
        let set_act = desc_sets[6];
        let set_down = desc_sets[7];

        // Bind storage buffers
        let bind_set = |device: &Device, set: vk::DescriptorSet, b0: vk::Buffer, s0: vk::DeviceSize, b1: vk::Buffer, s1: vk::DeviceSize, b2: vk::Buffer, s2: vk::DeviceSize| {
            let buffer_infos = [
                vk::DescriptorBufferInfo::builder().buffer(b0).offset(0).range(s0).build(),
                vk::DescriptorBufferInfo::builder().buffer(b1).offset(0).range(s1).build(),
                vk::DescriptorBufferInfo::builder().buffer(b2).offset(0).range(s2).build(),
            ];
            let writes = [
                vk::WriteDescriptorSet::builder().dst_set(set).dst_binding(0).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&buffer_infos[0..1]).build(),
                vk::WriteDescriptorSet::builder().dst_set(set).dst_binding(1).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&buffer_infos[1..2]).build(),
                vk::WriteDescriptorSet::builder().dst_set(set).dst_binding(2).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&buffer_infos[2..3]).build(),
            ];
            unsafe { device.update_descriptor_sets(&writes, &[]) };
        };

        bind_set(&device, set_q, inputs_2304_buffer, 2304 * 4, weight_q_buffer, weight_q.len() as vk::DeviceSize, out_2304_a_buffer, 2304 * 4);
        bind_set(&device, set_k, inputs_2304_buffer, 2304 * 4, weight_k_buffer, weight_k.len() as vk::DeviceSize, out_256_k_buffer, 256 * 4);
        bind_set(&device, set_v, inputs_2304_buffer, 2304 * 4, weight_v_buffer, weight_v.len() as vk::DeviceSize, out_256_v_buffer, 256 * 4);
        bind_set(&device, set_o, inputs_2304_buffer, 2304 * 4, weight_o_buffer, weight_o.len() as vk::DeviceSize, out_2304_b_buffer, 2304 * 4);
        bind_set(&device, set_gate, inputs_2304_buffer, 2304 * 4, weight_gate_buffer, weight_gate.len() as vk::DeviceSize, out_11008_gate_buffer, 11008 * 4);
        bind_set(&device, set_up, inputs_2304_buffer, 2304 * 4, weight_up_buffer, weight_up.len() as vk::DeviceSize, out_11008_up_buffer, 11008 * 4);
        bind_set(&device, set_act, out_11008_gate_buffer, 11008 * 4, out_11008_up_buffer, 11008 * 4, inputs_11008_buffer, 11008 * 4);
        bind_set(&device, set_down, inputs_11008_buffer, 11008 * 4, weight_down_buffer, weight_down.len() as vk::DeviceSize, out_2304_a_buffer, 2304 * 4);

        // 7. Command Buffer Recording
        let command_pool_info = vk::CommandPoolCreateInfo::builder().queue_family_index(driver.queue_family_index).flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = unsafe { device.create_command_pool(&command_pool_info, None)? };
        
        let alloc_info = vk::CommandBufferAllocateInfo::builder().command_pool(command_pool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(1);
        let command_buffers = unsafe { device.allocate_command_buffers(&alloc_info)? };
        let command_buffer = command_buffers[0];

        let begin_info = vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::SIMULTANEOUS_USE);
        unsafe {
            device.begin_command_buffer(command_buffer, &begin_info)?;
            
            // Dispatch Q, K, V, O, Gate, Up (concurrently, all bind pipeline_int4)
            let dispatch_gemv = |cmd: vk::CommandBuffer, pipe: vk::Pipeline, layout: vk::PipelineLayout, set: vk::DescriptorSet, k: u32, n: u32| {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, pipe);
                device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::COMPUTE, layout, 0, &[set], &[]);
                let params = [k, n];
                let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
                device.cmd_push_constants(cmd, layout, vk::ShaderStageFlags::COMPUTE, 0, params_bytes);
                let workgroups = (n + 63) / 64;
                device.cmd_dispatch(cmd, workgroups, 1, 1);
            };

            dispatch_gemv(command_buffer, pipeline_int4, pipeline_layout, set_q, 2304, 2304);
            dispatch_gemv(command_buffer, pipeline_int4, pipeline_layout, set_k, 2304, 256);
            dispatch_gemv(command_buffer, pipeline_int4, pipeline_layout, set_v, 2304, 256);
            dispatch_gemv(command_buffer, pipeline_int4, pipeline_layout, set_o, 2304, 2304);
            dispatch_gemv(command_buffer, pipeline_int4, pipeline_layout, set_gate, 2304, 11008);
            dispatch_gemv(command_buffer, pipeline_int4, pipeline_layout, set_up, 2304, 11008);

            // Pipeline Barrier: Wait for Gate and Up writes to finish
            let barriers = [
                vk::BufferMemoryBarrier::builder()
                    .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .buffer(out_11008_gate_buffer)
                    .offset(0)
                    .size(vk::WHOLE_SIZE)
                    .build(),
                vk::BufferMemoryBarrier::builder()
                    .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .buffer(out_11008_up_buffer)
                    .offset(0)
                    .size(vk::WHOLE_SIZE)
                    .build(),
            ];
            device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &barriers,
                &[],
            );

            // Dispatch Qwen Activation
            device.cmd_bind_pipeline(command_buffer, vk::PipelineBindPoint::COMPUTE, pipeline_act);
            device.cmd_bind_descriptor_sets(command_buffer, vk::PipelineBindPoint::COMPUTE, pipeline_layout, 0, &[set_act], &[]);
            let act_workgroups = (11008 + 63) / 64;
            device.cmd_dispatch(command_buffer, act_workgroups, 1, 1);

            // Pipeline Barrier: Wait for Activation writes to finish
            let barrier_act = [
                vk::BufferMemoryBarrier::builder()
                    .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .buffer(inputs_11008_buffer)
                    .offset(0)
                    .size(vk::WHOLE_SIZE)
                    .build(),
            ];
            device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &barrier_act,
                &[],
            );

            // Dispatch GEMV Down
            dispatch_gemv(command_buffer, pipeline_int4, pipeline_layout, set_down, 11008, 2304);
            
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

    pub fn run(&self, input_bytes: &[u8], output_floats: &mut [f32]) -> Result<f64, Box<dyn std::error::Error>> {
        // Stream input data
        unsafe {
            std::ptr::copy_nonoverlapping(input_bytes.as_ptr(), self.inputs_2304_ptr as *mut u8, input_bytes.len());
        }

        // Submit once
        let start = Instant::now();
        let command_buffers = [self.command_buffer];
        let submit_info = vk::SubmitInfo::builder().command_buffers(&command_buffers);
        unsafe {
            self.device.reset_fences(&[self.fence])?;
            self.device.queue_submit(self.queue, &[submit_info.build()], self.fence)?;
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        let duration_us = start.elapsed().as_micros() as f64;

        // Stream output data
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
            
            let destroy_buffer = |device: &Device, buffer: vk::Buffer, memory: vk::DeviceMemory, mapped: bool| {
                if mapped {
                    device.unmap_memory(memory);
                }
                device.free_memory(memory, None);
                device.destroy_buffer(buffer, None);
            };

            destroy_buffer(&self.device, self.inputs_2304_buffer, self.inputs_2304_memory, true);
            destroy_buffer(&self.device, self.out_2304_a_buffer, self.out_2304_a_memory, true);
            destroy_buffer(&self.device, self.out_2304_b_buffer, self.out_2304_b_memory, false);
            destroy_buffer(&self.device, self.out_256_k_buffer, self.out_256_k_memory, false);
            destroy_buffer(&self.device, self.out_256_v_buffer, self.out_256_v_memory, false);
            destroy_buffer(&self.device, self.out_11008_gate_buffer, self.out_11008_gate_memory, false);
            destroy_buffer(&self.device, self.out_11008_up_buffer, self.out_11008_up_memory, false);
            destroy_buffer(&self.device, self.inputs_11008_buffer, self.inputs_11008_memory, false);

            destroy_buffer(&self.device, self.weight_q_buffer, self.weight_q_memory, false);
            destroy_buffer(&self.device, self.weight_k_buffer, self.weight_k_memory, false);
            destroy_buffer(&self.device, self.weight_v_buffer, self.weight_v_memory, false);
            destroy_buffer(&self.device, self.weight_o_buffer, self.weight_o_memory, false);
            destroy_buffer(&self.device, self.weight_gate_buffer, self.weight_gate_memory, false);
            destroy_buffer(&self.device, self.weight_up_buffer, self.weight_up_memory, false);
            destroy_buffer(&self.device, self.weight_down_buffer, self.weight_down_memory, false);

            self.device.destroy_pipeline(self.pipeline_int4, None);
            self.device.destroy_pipeline(self.pipeline_act, None);
            self.device.destroy_pipeline_layout(self.pipeline_layout, None);
            self.device.destroy_descriptor_set_layout(self.desc_set_layout, None);
            
            self.device.destroy_shader_module(self.shader_int4, None);
            self.device.destroy_shader_module(self.shader_act, None);
        }
    }
}

pub struct VulkanBitNetLayer {
    pub device: Device,
    pub queue: vk::Queue,
    
    // Shader Modules
    pub shader_ternary: vk::ShaderModule,
    pub shader_act: vk::ShaderModule,
    
    // Descriptor Set Layout
    pub desc_set_layout: vk::DescriptorSetLayout,
    pub pipeline_layout: vk::PipelineLayout,
    
    // Pipelines
    pub pipeline_ternary: vk::Pipeline,
    pub pipeline_act: vk::Pipeline,
    
    // Buffers & Memory
    pub inputs_3200_buffer: vk::Buffer,
    pub inputs_3200_memory: vk::DeviceMemory,
    pub inputs_3200_ptr: *mut std::ffi::c_void,
    
    pub out_3200_down_buffer: vk::Buffer,
    pub out_3200_down_memory: vk::DeviceMemory,
    pub out_3200_down_ptr: *mut std::ffi::c_void,
    
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
    
    // Weights
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
    
    // Descriptors
    pub desc_pool: vk::DescriptorPool,
    pub desc_sets: Vec<vk::DescriptorSet>,
    
    // Execution
    pub command_pool: vk::CommandPool,
    pub command_buffer: vk::CommandBuffer,
    pub fence: vk::Fence,
}

impl VulkanBitNetLayer {
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

        // 1. Create Shaders
        let shader_info_ternary = vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::TERNARY_SPV);
        let shader_ternary = unsafe { device.create_shader_module(&shader_info_ternary, None)? };
        
        let shader_info_act = vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::ACT_BITNET_SPV);
        let shader_act = unsafe { device.create_shader_module(&shader_info_act, None)? };

        // 2. Create Descriptor Layout
        let bindings = [
            vk::DescriptorSetLayoutBinding::builder().binding(0).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
            vk::DescriptorSetLayoutBinding::builder().binding(1).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
            vk::DescriptorSetLayoutBinding::builder().binding(2).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
        ];
        let layout_info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);
        let desc_set_layout = unsafe { device.create_descriptor_set_layout(&layout_info, None)? };

        // 3. Create Pipeline Layout
        let push_constant_ranges = [
            vk::PushConstantRange::builder().stage_flags(vk::ShaderStageFlags::COMPUTE).offset(0).size(8).build(),
        ];
        let layouts = [desc_set_layout];
        let pipeline_layout_info = vk::PipelineLayoutCreateInfo::builder().set_layouts(&layouts).push_constant_ranges(&push_constant_ranges);
        let pipeline_layout = unsafe { device.create_pipeline_layout(&pipeline_layout_info, None)? };

        // 4. Create Compute Pipelines
        let main_entry = CString::new("main")?;
        
        let stage_info_ternary = vk::PipelineShaderStageCreateInfo::builder().stage(vk::ShaderStageFlags::COMPUTE).module(shader_ternary).name(&main_entry);
        let pipeline_create_info_ternary = vk::ComputePipelineCreateInfo::builder().stage(stage_info_ternary.build()).layout(pipeline_layout);
        
        let stage_info_act = vk::PipelineShaderStageCreateInfo::builder().stage(vk::ShaderStageFlags::COMPUTE).module(shader_act).name(&main_entry);
        let pipeline_create_info_act = vk::ComputePipelineCreateInfo::builder().stage(stage_info_act.build()).layout(pipeline_layout);
        
        let pipelines = unsafe {
            device.create_compute_pipelines(vk::PipelineCache::null(), &[pipeline_create_info_ternary.build(), pipeline_create_info_act.build()], None).map_err(|(_, e)| e)?
        };
        let pipeline_ternary = pipelines[0];
        let pipeline_act = pipelines[1];

        // 5. Create Buffers
        let (inputs_3200_buffer, inputs_3200_memory, inputs_3200_ptr) = create_coherent_buffer(&device, &instance, physical_device, (3200 / 16) * 4, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (out_3200_down_buffer, out_3200_down_memory, out_3200_down_ptr) = create_coherent_buffer(&device, &instance, physical_device, 3200 * 4, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        
        let (out_3200_q_buffer, out_3200_q_memory, _) = create_coherent_buffer(&device, &instance, physical_device, 3200 * 4, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (out_3200_k_buffer, out_3200_k_memory, _) = create_coherent_buffer(&device, &instance, physical_device, 3200 * 4, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (out_3200_v_buffer, out_3200_v_memory, _) = create_coherent_buffer(&device, &instance, physical_device, 3200 * 4, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (out_3200_o_buffer, out_3200_o_memory, _) = create_coherent_buffer(&device, &instance, physical_device, 3200 * 4, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (out_8640_gate_buffer, out_8640_gate_memory, _) = create_coherent_buffer(&device, &instance, physical_device, 8640 * 4, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (out_8640_up_buffer, out_8640_up_memory, _) = create_coherent_buffer(&device, &instance, physical_device, 8640 * 4, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (inputs_8640_buffer, inputs_8640_memory, _) = create_coherent_buffer(&device, &instance, physical_device, (8640 / 16) * 4, vk::BufferUsageFlags::STORAGE_BUFFER)?;

        // Weights
        let weight_q_packed = pack_weights_uvec4(weight_q, 3200, 3200);
        let weight_k_packed = pack_weights_uvec4(weight_k, 3200, 3200);
        let weight_v_packed = pack_weights_uvec4(weight_v, 3200, 3200);
        let weight_o_packed = pack_weights_uvec4(weight_o, 3200, 3200);
        let weight_gate_packed = pack_weights_uvec4(weight_gate, 3200, 8640);
        let weight_up_packed = pack_weights_uvec4(weight_up, 3200, 8640);
        let weight_down_packed = pack_weights_uvec4(weight_down, 8640, 3200);

        let (weight_q_buffer, weight_q_memory) = create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, weight_q_packed.len() as vk::DeviceSize, &weight_q_packed)?;
        let (weight_k_buffer, weight_k_memory) = create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, weight_k_packed.len() as vk::DeviceSize, &weight_k_packed)?;
        let (weight_v_buffer, weight_v_memory) = create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, weight_v_packed.len() as vk::DeviceSize, &weight_v_packed)?;
        let (weight_o_buffer, weight_o_memory) = create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, weight_o_packed.len() as vk::DeviceSize, &weight_o_packed)?;
        let (weight_gate_buffer, weight_gate_memory) = create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, weight_gate_packed.len() as vk::DeviceSize, &weight_gate_packed)?;
        let (weight_up_buffer, weight_up_memory) = create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, weight_up_packed.len() as vk::DeviceSize, &weight_up_packed)?;
        let (weight_down_buffer, weight_down_memory) = create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, weight_down_packed.len() as vk::DeviceSize, &weight_down_packed)?;

        // 6. Descriptor Pool
        let pool_sizes = [
            vk::DescriptorPoolSize::builder().ty(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(24).build(),
        ];
        let pool_info = vk::DescriptorPoolCreateInfo::builder().max_sets(8).pool_sizes(&pool_sizes);
        let desc_pool = unsafe { device.create_descriptor_pool(&pool_info, None)? };

        // Allocate 8 sets
        let layouts_vec = vec![desc_set_layout; 8];
        let alloc_info = vk::DescriptorSetAllocateInfo::builder().descriptor_pool(desc_pool).set_layouts(&layouts_vec);
        let desc_sets = unsafe { device.allocate_descriptor_sets(&alloc_info)? };
        
        let set_q = desc_sets[0];
        let set_k = desc_sets[1];
        let set_v = desc_sets[2];
        let set_o = desc_sets[3];
        let set_gate = desc_sets[4];
        let set_up = desc_sets[5];
        let set_act = desc_sets[6];
        let set_down = desc_sets[7];

        // Bind storage buffers
        let bind_set = |device: &Device, set: vk::DescriptorSet, b0: vk::Buffer, s0: vk::DeviceSize, b1: vk::Buffer, s1: vk::DeviceSize, b2: vk::Buffer, s2: vk::DeviceSize| {
            let buffer_infos = [
                vk::DescriptorBufferInfo::builder().buffer(b0).offset(0).range(s0).build(),
                vk::DescriptorBufferInfo::builder().buffer(b1).offset(0).range(s1).build(),
                vk::DescriptorBufferInfo::builder().buffer(b2).offset(0).range(s2).build(),
            ];
            let writes = [
                vk::WriteDescriptorSet::builder().dst_set(set).dst_binding(0).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&buffer_infos[0..1]).build(),
                vk::WriteDescriptorSet::builder().dst_set(set).dst_binding(1).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&buffer_infos[1..2]).build(),
                vk::WriteDescriptorSet::builder().dst_set(set).dst_binding(2).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&buffer_infos[2..3]).build(),
            ];
            unsafe { device.update_descriptor_sets(&writes, &[]) };
        };

        bind_set(&device, set_q, inputs_3200_buffer, (3200 / 16) * 4, weight_q_buffer, weight_q.len() as vk::DeviceSize, out_3200_q_buffer, 3200 * 4);
        bind_set(&device, set_k, inputs_3200_buffer, (3200 / 16) * 4, weight_k_buffer, weight_k.len() as vk::DeviceSize, out_3200_k_buffer, 3200 * 4);
        bind_set(&device, set_v, inputs_3200_buffer, (3200 / 16) * 4, weight_v_buffer, weight_v.len() as vk::DeviceSize, out_3200_v_buffer, 3200 * 4);
        bind_set(&device, set_o, inputs_3200_buffer, (3200 / 16) * 4, weight_o_buffer, weight_o.len() as vk::DeviceSize, out_3200_o_buffer, 3200 * 4);
        bind_set(&device, set_gate, inputs_3200_buffer, (3200 / 16) * 4, weight_gate_buffer, weight_gate.len() as vk::DeviceSize, out_8640_gate_buffer, 8640 * 4);
        bind_set(&device, set_up, inputs_3200_buffer, (3200 / 16) * 4, weight_up_buffer, weight_up.len() as vk::DeviceSize, out_8640_up_buffer, 8640 * 4);
        bind_set(&device, set_act, out_8640_gate_buffer, 8640 * 4, out_8640_up_buffer, 8640 * 4, inputs_8640_buffer, (8640 / 16) * 4);
        bind_set(&device, set_down, inputs_8640_buffer, (8640 / 16) * 4, weight_down_buffer, weight_down.len() as vk::DeviceSize, out_3200_down_buffer, 3200 * 4);

        // 7. Command Buffer Recording
        let command_pool_info = vk::CommandPoolCreateInfo::builder().queue_family_index(driver.queue_family_index).flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = unsafe { device.create_command_pool(&command_pool_info, None)? };
        
        let alloc_info = vk::CommandBufferAllocateInfo::builder().command_pool(command_pool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(1);
        let command_buffers = unsafe { device.allocate_command_buffers(&alloc_info)? };
        let command_buffer = command_buffers[0];

        let begin_info = vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::SIMULTANEOUS_USE);
        unsafe {
            device.begin_command_buffer(command_buffer, &begin_info)?;
            
            // Dispatch Q, K, V, O, Gate, Up (concurrently, all bind pipeline_ternary)
            let dispatch_gemv = |cmd: vk::CommandBuffer, pipe: vk::Pipeline, layout: vk::PipelineLayout, set: vk::DescriptorSet, k: u32, n: u32| {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, pipe);
                device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::COMPUTE, layout, 0, &[set], &[]);
                let params = [k, n];
                let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
                device.cmd_push_constants(cmd, layout, vk::ShaderStageFlags::COMPUTE, 0, params_bytes);
                let workgroups = (n + 255) / 256;
                device.cmd_dispatch(cmd, workgroups, 1, 1);
            };

            dispatch_gemv(command_buffer, pipeline_ternary, pipeline_layout, set_q, 3200, 3200);
            dispatch_gemv(command_buffer, pipeline_ternary, pipeline_layout, set_k, 3200, 3200);
            dispatch_gemv(command_buffer, pipeline_ternary, pipeline_layout, set_v, 3200, 3200);
            dispatch_gemv(command_buffer, pipeline_ternary, pipeline_layout, set_o, 3200, 3200);
            dispatch_gemv(command_buffer, pipeline_ternary, pipeline_layout, set_gate, 3200, 8640);
            dispatch_gemv(command_buffer, pipeline_ternary, pipeline_layout, set_up, 3200, 8640);

            // Pipeline Barrier: Wait for Gate and Up writes to finish
            let barriers = [
                vk::BufferMemoryBarrier::builder()
                    .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .buffer(out_8640_gate_buffer)
                    .offset(0)
                    .size(vk::WHOLE_SIZE)
                    .build(),
                vk::BufferMemoryBarrier::builder()
                    .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .buffer(out_8640_up_buffer)
                    .offset(0)
                    .size(vk::WHOLE_SIZE)
                    .build(),
            ];
            device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &barriers,
                &[],
            );

            // Dispatch BitNet Activation
            device.cmd_bind_pipeline(command_buffer, vk::PipelineBindPoint::COMPUTE, pipeline_act);
            device.cmd_bind_descriptor_sets(command_buffer, vk::PipelineBindPoint::COMPUTE, pipeline_layout, 0, &[set_act], &[]);
            let act_workgroups = (540 + 63) / 64;
            device.cmd_dispatch(command_buffer, act_workgroups, 1, 1);

            // Pipeline Barrier: Wait for Activation writes to finish
            let barrier_act = [
                vk::BufferMemoryBarrier::builder()
                    .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .buffer(inputs_8640_buffer)
                    .offset(0)
                    .size(vk::WHOLE_SIZE)
                    .build(),
            ];
            device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &barrier_act,
                &[],
            );

            // Dispatch GEMV Down
            dispatch_gemv(command_buffer, pipeline_ternary, pipeline_layout, set_down, 8640, 3200);
            
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
            out_3200_down_buffer,
            out_3200_down_memory,
            out_3200_down_ptr,
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

    pub fn run(&self, input_bytes: &[u8], output_floats: &mut [f32]) -> Result<f64, Box<dyn std::error::Error>> {
        // Stream input data
        unsafe {
            std::ptr::copy_nonoverlapping(input_bytes.as_ptr(), self.inputs_3200_ptr as *mut u8, input_bytes.len());
        }

        // Submit once
        let start = Instant::now();
        let command_buffers = [self.command_buffer];
        let submit_info = vk::SubmitInfo::builder().command_buffers(&command_buffers);
        unsafe {
            self.device.reset_fences(&[self.fence])?;
            self.device.queue_submit(self.queue, &[submit_info.build()], self.fence)?;
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        let duration_us = start.elapsed().as_micros() as f64;

        // Stream output data
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
            
            let destroy_buffer = |device: &Device, buffer: vk::Buffer, memory: vk::DeviceMemory, mapped: bool| {
                if mapped {
                    device.unmap_memory(memory);
                }
                device.free_memory(memory, None);
                device.destroy_buffer(buffer, None);
            };

            destroy_buffer(&self.device, self.inputs_3200_buffer, self.inputs_3200_memory, true);
            destroy_buffer(&self.device, self.out_3200_down_buffer, self.out_3200_down_memory, true);
            destroy_buffer(&self.device, self.out_3200_q_buffer, self.out_3200_q_memory, false);
            destroy_buffer(&self.device, self.out_3200_k_buffer, self.out_3200_k_memory, false);
            destroy_buffer(&self.device, self.out_3200_v_buffer, self.out_3200_v_memory, false);
            destroy_buffer(&self.device, self.out_3200_o_buffer, self.out_3200_o_memory, false);
            destroy_buffer(&self.device, self.out_8640_gate_buffer, self.out_8640_gate_memory, false);
            destroy_buffer(&self.device, self.out_8640_up_buffer, self.out_8640_up_memory, false);
            destroy_buffer(&self.device, self.inputs_8640_buffer, self.inputs_8640_memory, false);

            destroy_buffer(&self.device, self.weight_q_buffer, self.weight_q_memory, false);
            destroy_buffer(&self.device, self.weight_k_buffer, self.weight_k_memory, false);
            destroy_buffer(&self.device, self.weight_v_buffer, self.weight_v_memory, false);
            destroy_buffer(&self.device, self.weight_o_buffer, self.weight_o_memory, false);
            destroy_buffer(&self.device, self.weight_gate_buffer, self.weight_gate_memory, false);
            destroy_buffer(&self.device, self.weight_up_buffer, self.weight_up_memory, false);
            destroy_buffer(&self.device, self.weight_down_buffer, self.weight_down_memory, false);


            self.device.destroy_pipeline(self.pipeline_ternary, None);
            self.device.destroy_pipeline(self.pipeline_act, None);
            self.device.destroy_pipeline_layout(self.pipeline_layout, None);
            self.device.destroy_descriptor_set_layout(self.desc_set_layout, None);
            
            self.device.destroy_shader_module(self.shader_ternary, None);
            self.device.destroy_shader_module(self.shader_act, None);
        }
    }
}

// --- V.E.L.O.C.I.T.Y. NDA (Decomposed Bitmap Index) Implementation ---

pub fn pack_weights_nda(src: &[u8], k: usize, n: usize) -> (Vec<u8>, Vec<u8>) {
    let src_u32 = unsafe {
        std::slice::from_raw_parts(src.as_ptr() as *const u32, src.len() / 4)
    };
    
    let num_col_words = k / 32;
    let mut active_dest = vec![0u32; num_col_words * n];
    let mut pos_dest = vec![0u32; num_col_words * n];
    
    for row in 0..n {
        for col_word in 0..num_col_words {
            let src_col_0 = col_word * 2;
            let src_col_1 = col_word * 2 + 1;
            
            let w0 = src_u32[src_col_0 * n + row];
            let w1 = src_u32[src_col_1 * n + row];
            
            let mut act_word = 0u32;
            let mut pos_word = 0u32;
            
            for j in 0..16 {
                let pair = (w0 >> (j * 2)) & 3;
                if pair != 0 {
                    act_word |= 1 << j;
                    if pair == 3 {
                        pos_word |= 1 << j;
                    }
                }
            }
            for j in 0..16 {
                let pair = (w1 >> (j * 2)) & 3;
                if pair != 0 {
                    act_word |= 1 << (16 + j);
                    if pair == 3 {
                        pos_word |= 1 << (16 + j);
                    }
                }
            }
            
            let dest_idx = col_word * n + row;
            active_dest[dest_idx] = act_word;
            pos_dest[dest_idx] = pos_word;
        }
    }
    
    let num_col_groups_4 = num_col_words / 4;
    let mut active_packed = vec![0u32; num_col_words * n];
    let mut pos_packed = vec![0u32; num_col_words * n];
    
    for cg4 in 0..num_col_groups_4 {
        for row in 0..n {
            for offset in 0..4 {
                let cg = cg4 * 4 + offset;
                let src_idx = cg * n + row;
                let dest_idx = cg4 * n * 4 + row * 4 + offset;
                active_packed[dest_idx] = active_dest[src_idx];
                pos_packed[dest_idx] = pos_dest[src_idx];
            }
        }
    }
    
    let active_bytes = unsafe {
        let bytes_ptr = active_packed.as_ptr() as *const u8;
        std::slice::from_raw_parts(bytes_ptr, active_packed.len() * 4).to_vec()
    };
    
    let pos_bytes = unsafe {
        let bytes_ptr = pos_packed.as_ptr() as *const u8;
        std::slice::from_raw_parts(bytes_ptr, pos_packed.len() * 4).to_vec()
    };
    
    (active_bytes, pos_bytes)
}

pub fn pack_inputs_nda(src: &[u32]) -> (Vec<u32>, Vec<u32>) {
    let k = src.len() * 16;
    let num_col_words = k / 32;
    let mut active_dest = vec![0u32; num_col_words];
    let mut pos_dest = vec![0u32; num_col_words];
    
    for col_word in 0..num_col_words {
        let w0 = src[col_word * 2];
        let w1 = src[col_word * 2 + 1];
        
        let mut act_word = 0u32;
        let mut pos_word = 0u32;
        
        for j in 0..16 {
            let pair = (w0 >> (j * 2)) & 3;
            if pair != 0 {
                act_word |= 1 << j;
                if pair == 3 {
                    pos_word |= 1 << j;
                }
            }
        }
        for j in 0..16 {
            let pair = (w1 >> (j * 2)) & 3;
            if pair != 0 {
                act_word |= 1 << (16 + j);
                if pair == 3 {
                    pos_word |= 1 << (16 + j);
                }
            }
        }
        active_dest[col_word] = act_word;
        pos_dest[col_word] = pos_word;
    }
    
    (active_dest, pos_dest)
}

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
    pub fn record_dispatch(
        &self,
        cmd: vk::CommandBuffer,
    ) {
        unsafe {
            self.device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, self.compute_pipeline);
            self.device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::COMPUTE, self.pipeline_layout, 0, &[self.desc_set], &[]);
            
            let params = [self.k, self.n];
            let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
            self.device.cmd_push_constants(cmd, self.pipeline_layout, vk::ShaderStageFlags::COMPUTE, 0, params_bytes);
            
            let workgroup_count_y = (self.n + 15) / 16;
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

        // 1. Create Shader Module
        let shader_info = vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::NDA_SPV);
        let shader_module = unsafe { device.create_shader_module(&shader_info, None)? };

        // 2. Create Descriptor Set Layout
        let bindings = [
            vk::DescriptorSetLayoutBinding::builder().binding(0).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
            vk::DescriptorSetLayoutBinding::builder().binding(1).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
            vk::DescriptorSetLayoutBinding::builder().binding(2).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
            vk::DescriptorSetLayoutBinding::builder().binding(3).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
            vk::DescriptorSetLayoutBinding::builder().binding(4).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
        ];
        let layout_info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);
        let desc_set_layout = unsafe { device.create_descriptor_set_layout(&layout_info, None)? };

        // 3. Create Pipeline Layout with Push Constants
        let push_constant_ranges = [
            vk::PushConstantRange::builder().stage_flags(vk::ShaderStageFlags::COMPUTE).offset(0).size(8).build(),
        ];
        let layouts = [desc_set_layout];
        let pipeline_layout_info = vk::PipelineLayoutCreateInfo::builder().set_layouts(&layouts).push_constant_ranges(&push_constant_ranges);
        let pipeline_layout = unsafe { device.create_pipeline_layout(&pipeline_layout_info, None)? };

        // 4. Create Pipeline
        let main_entry = CString::new("main")?;
        let stage_info = vk::PipelineShaderStageCreateInfo::builder().stage(vk::ShaderStageFlags::COMPUTE).module(shader_module).name(&main_entry);
        let pipeline_create_info = vk::ComputePipelineCreateInfo::builder().stage(stage_info.build()).layout(pipeline_layout);
        let compute_pipelines = unsafe {
            device.create_compute_pipelines(vk::PipelineCache::null(), &[pipeline_create_info.build()], None).map_err(|(_, e)| e)?
        };
        let compute_pipeline = compute_pipelines[0];

        // 5. Pack Weights & Create Buffers
        let (active_w_bytes, pos_w_bytes) = pack_weights_nda(weight_bytes, k as usize, n as usize);
        
        let (input_active_buffer, input_active_memory, input_active_ptr) = create_coherent_buffer(&device, &instance, physical_device, (k / 8) as vk::DeviceSize, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (input_pos_buffer, input_pos_memory, input_pos_ptr) = create_coherent_buffer(&device, &instance, physical_device, (k / 8) as vk::DeviceSize, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        
        let (weight_active_buffer, weight_active_memory) = create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, active_w_bytes.len() as vk::DeviceSize, &active_w_bytes)?;
        let (weight_pos_buffer, weight_pos_memory) = create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, pos_w_bytes.len() as vk::DeviceSize, &pos_w_bytes)?;
        
        let (output_buffer, output_memory, output_ptr) = create_coherent_buffer(&device, &instance, physical_device, (n * 4) as vk::DeviceSize, vk::BufferUsageFlags::STORAGE_BUFFER)?;

        // 6. Create Descriptor Pool & Allocate Set
        let pool_sizes = [
            vk::DescriptorPoolSize::builder().ty(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(5).build(),
        ];
        let pool_info = vk::DescriptorPoolCreateInfo::builder().max_sets(1).pool_sizes(&pool_sizes);
        let desc_pool = unsafe { device.create_descriptor_pool(&pool_info, None)? };
        
        let desc_set = unsafe { device.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::builder().descriptor_pool(desc_pool).set_layouts(&[desc_set_layout]))?[0] };

        // Write descriptor sets
        let buffer_infos = [
            vk::DescriptorBufferInfo::builder().buffer(input_active_buffer).offset(0).range((k / 8) as vk::DeviceSize).build(),
            vk::DescriptorBufferInfo::builder().buffer(input_pos_buffer).offset(0).range((k / 8) as vk::DeviceSize).build(),
            vk::DescriptorBufferInfo::builder().buffer(weight_active_buffer).offset(0).range(active_w_bytes.len() as vk::DeviceSize).build(),
            vk::DescriptorBufferInfo::builder().buffer(weight_pos_buffer).offset(0).range(pos_w_bytes.len() as vk::DeviceSize).build(),
            vk::DescriptorBufferInfo::builder().buffer(output_buffer).offset(0).range((n * 4) as vk::DeviceSize).build(),
        ];
        let writes = [
            vk::WriteDescriptorSet::builder().dst_set(desc_set).dst_binding(0).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&buffer_infos[0..1]).build(),
            vk::WriteDescriptorSet::builder().dst_set(desc_set).dst_binding(1).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&buffer_infos[1..2]).build(),
            vk::WriteDescriptorSet::builder().dst_set(desc_set).dst_binding(2).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&buffer_infos[2..3]).build(),
            vk::WriteDescriptorSet::builder().dst_set(desc_set).dst_binding(3).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&buffer_infos[3..4]).build(),
            vk::WriteDescriptorSet::builder().dst_set(desc_set).dst_binding(4).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&buffer_infos[4..5]).build(),
        ];
        unsafe { device.update_descriptor_sets(&writes, &[]) };

        // 7. Command Buffer
        let command_pool_info = vk::CommandPoolCreateInfo::builder().queue_family_index(driver.queue_family_index).flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = unsafe { device.create_command_pool(&command_pool_info, None)? };
        let command_buffers = unsafe { device.allocate_command_buffers(&vk::CommandBufferAllocateInfo::builder().command_pool(command_pool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(1))? };
        let command_buffer = command_buffers[0];

        let begin_info = vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::SIMULTANEOUS_USE);
        unsafe {
            device.begin_command_buffer(command_buffer, &begin_info)?;
            device.cmd_bind_pipeline(command_buffer, vk::PipelineBindPoint::COMPUTE, compute_pipeline);
            device.cmd_bind_descriptor_sets(command_buffer, vk::PipelineBindPoint::COMPUTE, pipeline_layout, 0, &[desc_set], &[]);
            
            let params = [k, n];
            let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
            device.cmd_push_constants(command_buffer, pipeline_layout, vk::ShaderStageFlags::COMPUTE, 0, params_bytes);
            
            let workgroup_count = (n + 63) / 64;
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

        // 1. Create Shader Module
        let spv_code = match version {
            3 => crate::compiler::shaders::FP4_SPV,
            4 => crate::compiler::shaders::FP2_SPV,
            _ => crate::compiler::shaders::NDA_SPV,
        };
        let shader_info = vk::ShaderModuleCreateInfo::builder().code(spv_code);
        let shader_module = unsafe { device.create_shader_module(&shader_info, None)? };

        // 2. Create Descriptor Set Layout
        let bindings = if is_float_act {
            vec![
                vk::DescriptorSetLayoutBinding::builder().binding(0).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
                vk::DescriptorSetLayoutBinding::builder().binding(1).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
                vk::DescriptorSetLayoutBinding::builder().binding(2).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
                vk::DescriptorSetLayoutBinding::builder().binding(3).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
            ]
        } else {
            vec![
                vk::DescriptorSetLayoutBinding::builder().binding(0).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
                vk::DescriptorSetLayoutBinding::builder().binding(1).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
                vk::DescriptorSetLayoutBinding::builder().binding(2).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
                vk::DescriptorSetLayoutBinding::builder().binding(3).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
                vk::DescriptorSetLayoutBinding::builder().binding(4).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
            ]
        };
        let layout_info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);
        let desc_set_layout = unsafe { device.create_descriptor_set_layout(&layout_info, None)? };

        // 3. Create Pipeline Layout with Push Constants
        let push_constant_ranges = [
            vk::PushConstantRange::builder().stage_flags(vk::ShaderStageFlags::COMPUTE).offset(0).size(8).build(),
        ];
        let layouts = [desc_set_layout];
        let pipeline_layout_info = vk::PipelineLayoutCreateInfo::builder().set_layouts(&layouts).push_constant_ranges(&push_constant_ranges);
        let pipeline_layout = unsafe { device.create_pipeline_layout(&pipeline_layout_info, None)? };

        // 4. Create Pipeline
        let main_entry = CString::new("main")?;
        let stage_info = vk::PipelineShaderStageCreateInfo::builder().stage(vk::ShaderStageFlags::COMPUTE).module(shader_module).name(&main_entry);
        let pipeline_create_info = vk::ComputePipelineCreateInfo::builder().stage(stage_info.build()).layout(pipeline_layout);
        let compute_pipelines = unsafe {
            device.create_compute_pipelines(vk::PipelineCache::null(), &[pipeline_create_info.build()], None).map_err(|(_, e)| e)?
        };
        let compute_pipeline = compute_pipelines[0];

        // 5. Create Buffers
        let input_active_size = if is_float_act { (k * 4) as vk::DeviceSize } else { (k / 8) as vk::DeviceSize };
        let (input_active_buffer, input_active_memory, input_active_ptr) = if is_float_act {
            (driver.shared_input_buffer, driver.shared_input_memory, driver.shared_input_ptr)
        } else {
            create_coherent_buffer(&device, &instance, physical_device, input_active_size, vk::BufferUsageFlags::STORAGE_BUFFER)?
        };
        
        let (input_pos_buffer, input_pos_memory, input_pos_ptr) = if is_float_act {
            create_coherent_buffer(&device, &instance, physical_device, 4 as vk::DeviceSize, vk::BufferUsageFlags::STORAGE_BUFFER)?
        } else {
            create_coherent_buffer(&device, &instance, physical_device, (k / 8) as vk::DeviceSize, vk::BufferUsageFlags::STORAGE_BUFFER)?
        };
        
        let (weight_active_buffer, weight_active_memory) = create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, active_w_bytes.len() as vk::DeviceSize, active_w_bytes)?;
        let (weight_pos_buffer, weight_pos_memory) = create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, pos_w_bytes.len() as vk::DeviceSize, pos_w_bytes)?;
        
        let (output_buffer, output_memory, output_ptr) = create_coherent_buffer(&device, &instance, physical_device, (n * 4) as vk::DeviceSize, vk::BufferUsageFlags::STORAGE_BUFFER)?;

        // 6. Create Descriptor Pool & Allocate Set
        let num_bindings = if is_float_act { 4 } else { 5 };
        let pool_sizes = [
            vk::DescriptorPoolSize::builder().ty(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(num_bindings).build(),
        ];
        let pool_info = vk::DescriptorPoolCreateInfo::builder().max_sets(1).pool_sizes(&pool_sizes);
        let desc_pool = unsafe { device.create_descriptor_pool(&pool_info, None)? };
        
        let desc_set = unsafe { device.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::builder().descriptor_pool(desc_pool).set_layouts(&[desc_set_layout]))?[0] };

        // Write descriptor sets
        let buffer_infos = if is_float_act {
            vec![
                vk::DescriptorBufferInfo::builder().buffer(input_active_buffer).offset(0).range(input_active_size).build(),
                vk::DescriptorBufferInfo::builder().buffer(weight_active_buffer).offset(0).range(active_w_bytes.len() as vk::DeviceSize).build(),
                vk::DescriptorBufferInfo::builder().buffer(weight_pos_buffer).offset(0).range(pos_w_bytes.len() as vk::DeviceSize).build(),
                vk::DescriptorBufferInfo::builder().buffer(output_buffer).offset(0).range((n * 4) as vk::DeviceSize).build(),
            ]
        } else {
            vec![
                vk::DescriptorBufferInfo::builder().buffer(input_active_buffer).offset(0).range((k / 8) as vk::DeviceSize).build(),
                vk::DescriptorBufferInfo::builder().buffer(input_pos_buffer).offset(0).range((k / 8) as vk::DeviceSize).build(),
                vk::DescriptorBufferInfo::builder().buffer(weight_active_buffer).offset(0).range(active_w_bytes.len() as vk::DeviceSize).build(),
                vk::DescriptorBufferInfo::builder().buffer(weight_pos_buffer).offset(0).range(pos_w_bytes.len() as vk::DeviceSize).build(),
                vk::DescriptorBufferInfo::builder().buffer(output_buffer).offset(0).range((n * 4) as vk::DeviceSize).build(),
            ]
        };

        let mut writes = Vec::new();
        for i in 0..num_bindings as usize {
            writes.push(
                vk::WriteDescriptorSet::builder()
                    .dst_set(desc_set)
                    .dst_binding(i as u32)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&buffer_infos[i..i+1])
                    .build()
            );
        }
        unsafe { device.update_descriptor_sets(&writes, &[]) };

        // 7. Command Buffer
        let command_pool_info = vk::CommandPoolCreateInfo::builder().queue_family_index(driver.queue_family_index).flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = unsafe { device.create_command_pool(&command_pool_info, None)? };
        let command_buffers = unsafe { device.allocate_command_buffers(&vk::CommandBufferAllocateInfo::builder().command_pool(command_pool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(1))? };
        let command_buffer = command_buffers[0];

        let begin_info = vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::SIMULTANEOUS_USE);
        unsafe {
            device.begin_command_buffer(command_buffer, &begin_info)?;
            device.cmd_bind_pipeline(command_buffer, vk::PipelineBindPoint::COMPUTE, compute_pipeline);
            device.cmd_bind_descriptor_sets(command_buffer, vk::PipelineBindPoint::COMPUTE, pipeline_layout, 0, &[desc_set], &[]);
            
            let params = [k, n];
            let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
            device.cmd_push_constants(command_buffer, pipeline_layout, vk::ShaderStageFlags::COMPUTE, 0, params_bytes);
            
            let workgroup_count_y = (n + 15) / 16;
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
            std::ptr::copy_nonoverlapping(input_floats.as_ptr(), self.input_active_ptr as *mut f32, input_floats.len());
        }

        let start = Instant::now();
        unsafe {
            self.device.reset_fences(&[self.fence])?;
            self.device.queue_submit(self.queue, &[vk::SubmitInfo::builder().command_buffers(&[self.command_buffer]).build()], self.fence)?;
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        let duration_us = start.elapsed().as_micros() as f64;

        unsafe {
            std::ptr::copy_nonoverlapping(self.output_ptr as *const f32, output_floats.as_mut_ptr(), output_floats.len());
        }

        Ok(duration_us)
    }

    pub fn submit_async_float(&self, input_floats: &[f32]) -> Result<(), Box<dyn std::error::Error>> {
        unsafe {
            std::ptr::copy_nonoverlapping(input_floats.as_ptr(), self.input_active_ptr as *mut f32, input_floats.len());
            self.device.reset_fences(&[self.fence])?;
            self.device.queue_submit(self.queue, &[vk::SubmitInfo::builder().command_buffers(&[self.command_buffer]).build()], self.fence)?;
        }
        Ok(())
    }

    pub fn wait_and_copy_float(&self, output_floats: &mut [f32]) -> Result<f64, Box<dyn std::error::Error>> {
        let start = Instant::now();
        unsafe {
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        let duration_us = start.elapsed().as_micros() as f64;

        unsafe {
            std::ptr::copy_nonoverlapping(self.output_ptr as *const f32, output_floats.as_mut_ptr(), output_floats.len());
        }

        Ok(duration_us)
    }

    pub fn submit_async_float_no_copy(&self) -> Result<(), Box<dyn std::error::Error>> {
        unsafe {
            self.device.reset_fences(&[self.fence])?;
            self.device.queue_submit(self.queue, &[vk::SubmitInfo::builder().command_buffers(&[self.command_buffer]).build()], self.fence)?;
        }
        Ok(())
    }

    pub fn run_float_no_copy(&self, output_floats: &mut [f32]) -> Result<f64, Box<dyn std::error::Error>> {
        let start = Instant::now();
        unsafe {
            self.device.reset_fences(&[self.fence])?;
            self.device.queue_submit(self.queue, &[vk::SubmitInfo::builder().command_buffers(&[self.command_buffer]).build()], self.fence)?;
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        let duration_us = start.elapsed().as_micros() as f64;

        unsafe {
            std::ptr::copy_nonoverlapping(self.output_ptr as *const f32, output_floats.as_mut_ptr(), output_floats.len());
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
            std::ptr::copy_nonoverlapping(input_active.as_ptr(), self.input_active_ptr as *mut u8, input_active.len());
            std::ptr::copy_nonoverlapping(input_pos.as_ptr(), self.input_pos_ptr as *mut u8, input_pos.len());
        }

        let start = Instant::now();
        unsafe {
            self.device.reset_fences(&[self.fence])?;
            self.device.queue_submit(self.queue, &[vk::SubmitInfo::builder().command_buffers(&[self.command_buffer]).build()], self.fence)?;
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        let duration_us = start.elapsed().as_micros() as f64;

        unsafe {
            std::ptr::copy_nonoverlapping(self.output_ptr as *const f32, output_floats.as_mut_ptr(), output_floats.len());
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
            
            let destroy_buffer = |device: &Device, buffer: vk::Buffer, memory: vk::DeviceMemory, mapped: bool| {
                if mapped {
                    device.unmap_memory(memory);
                }
                device.free_memory(memory, None);
                device.destroy_buffer(buffer, None);
            };

            let is_float_act = self.version == 3 || self.version == 4;
            if !is_float_act {
                destroy_buffer(&self.device, self.input_active_buffer, self.input_active_memory, true);
            }
            destroy_buffer(&self.device, self.input_pos_buffer, self.input_pos_memory, true);
            destroy_buffer(&self.device, self.weight_active_buffer, self.weight_active_memory, false);
            destroy_buffer(&self.device, self.weight_pos_buffer, self.weight_pos_memory, false);
            destroy_buffer(&self.device, self.output_buffer, self.output_memory, true);

            self.device.destroy_pipeline(self.compute_pipeline, None);
            self.device.destroy_pipeline_layout(self.pipeline_layout, None);
            self.device.destroy_descriptor_set_layout(self.desc_set_layout, None);
            self.device.destroy_shader_module(self.shader_module, None);
        }
    }
}

pub struct VulkanNdaBitNetLayer {
    pub device: Device,
    pub queue: vk::Queue,
    
    pub shader_nda: vk::ShaderModule,
    pub shader_act: vk::ShaderModule,
    
    pub desc_set_layout: vk::DescriptorSetLayout,
    pub pipeline_layout: vk::PipelineLayout,
    pub pipeline_nda: vk::Pipeline,
    pub pipeline_act: vk::Pipeline,
    
    pub inputs_3200_active_buffer: vk::Buffer,
    pub inputs_3200_active_memory: vk::DeviceMemory,
    pub inputs_3200_active_ptr: *mut std::ffi::c_void,
    
    pub inputs_3200_pos_buffer: vk::Buffer,
    pub inputs_3200_pos_memory: vk::DeviceMemory,
    pub inputs_3200_pos_ptr: *mut std::ffi::c_void,
    
    pub out_3200_down_buffer: vk::Buffer,
    pub out_3200_down_memory: vk::DeviceMemory,
    pub out_3200_down_ptr: *mut std::ffi::c_void,
    
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
    pub desc_sets: Vec<vk::DescriptorSet>,
    pub command_pool: vk::CommandPool,
    pub command_buffer: vk::CommandBuffer,
    pub fence: vk::Fence,
}

impl VulkanNdaBitNetLayer {
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

        // 1. Create Shaders
        let shader_nda = unsafe { device.create_shader_module(&vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::NDA_SPV), None)? };
        let shader_act = unsafe { device.create_shader_module(&vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::ACT_NDA_SPV), None)? };

        // 2. Descriptor Layout
        let bindings = [
            vk::DescriptorSetLayoutBinding::builder().binding(0).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
            vk::DescriptorSetLayoutBinding::builder().binding(1).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
            vk::DescriptorSetLayoutBinding::builder().binding(2).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
            vk::DescriptorSetLayoutBinding::builder().binding(3).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
            vk::DescriptorSetLayoutBinding::builder().binding(4).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
        ];
        let desc_set_layout = unsafe { device.create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings), None)? };

        // 3. Pipeline Layout
        let push_constant_ranges = [
            vk::PushConstantRange::builder().stage_flags(vk::ShaderStageFlags::COMPUTE).offset(0).size(8).build(),
        ];
        let pipeline_layout = unsafe { device.create_pipeline_layout(&vk::PipelineLayoutCreateInfo::builder().set_layouts(&[desc_set_layout]).push_constant_ranges(&push_constant_ranges), None)? };

        // 4. Pipelines
        let main_entry = CString::new("main")?;
        let pipeline_nda = unsafe {
            device.create_compute_pipelines(vk::PipelineCache::null(), &[vk::ComputePipelineCreateInfo::builder().stage(vk::PipelineShaderStageCreateInfo::builder().stage(vk::ShaderStageFlags::COMPUTE).module(shader_nda).name(&main_entry).build()).layout(pipeline_layout).build()], None).map_err(|(_, e)| e)?[0]
        };
        let pipeline_act = unsafe {
            device.create_compute_pipelines(vk::PipelineCache::null(), &[vk::ComputePipelineCreateInfo::builder().stage(vk::PipelineShaderStageCreateInfo::builder().stage(vk::ShaderStageFlags::COMPUTE).module(shader_act).name(&main_entry).build()).layout(pipeline_layout).build()], None).map_err(|(_, e)| e)?[0]
        };

        // 5. Allocate Buffers
        let (inputs_3200_active_buffer, inputs_3200_active_memory, inputs_3200_active_ptr) = create_coherent_buffer(&device, &instance, physical_device, 3200 / 8, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (inputs_3200_pos_buffer, inputs_3200_pos_memory, inputs_3200_pos_ptr) = create_coherent_buffer(&device, &instance, physical_device, 3200 / 8, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        
        let (out_3200_down_buffer, out_3200_down_memory, out_3200_down_ptr) = create_coherent_buffer(&device, &instance, physical_device, 3200 * 4, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        
        let (out_3200_q_buffer, out_3200_q_memory, _) = create_coherent_buffer(&device, &instance, physical_device, 3200 * 4, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (out_3200_k_buffer, out_3200_k_memory, _) = create_coherent_buffer(&device, &instance, physical_device, 3200 * 4, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (out_3200_v_buffer, out_3200_v_memory, _) = create_coherent_buffer(&device, &instance, physical_device, 3200 * 4, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (out_3200_o_buffer, out_3200_o_memory, _) = create_coherent_buffer(&device, &instance, physical_device, 3200 * 4, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        
        let (out_8640_gate_buffer, out_8640_gate_memory, _) = create_coherent_buffer(&device, &instance, physical_device, 8640 * 4, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (out_8640_up_buffer, out_8640_up_memory, _) = create_coherent_buffer(&device, &instance, physical_device, 8640 * 4, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        
        let (inputs_8640_active_buffer, inputs_8640_active_memory, _) = create_coherent_buffer(&device, &instance, physical_device, 8640 / 8, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (inputs_8640_pos_buffer, inputs_8640_pos_memory, _) = create_coherent_buffer(&device, &instance, physical_device, 8640 / 8, vk::BufferUsageFlags::STORAGE_BUFFER)?;

        // Pack weights
        let create_nda_weight_buffers = |device: &Device, src: &[u8], k: usize, n: usize| -> Result<(vk::Buffer, vk::DeviceMemory, vk::Buffer, vk::DeviceMemory), Box<dyn std::error::Error>> {
            let (act_bytes, pos_bytes) = pack_weights_nda(src, k, n);
            let (ab, am) = create_device_local_buffer(device, &instance, physical_device, queue, driver.queue_family_index, act_bytes.len() as vk::DeviceSize, &act_bytes)?;
            let (pb, pm) = create_device_local_buffer(device, &instance, physical_device, queue, driver.queue_family_index, pos_bytes.len() as vk::DeviceSize, &pos_bytes)?;
            Ok((ab, am, pb, pm))
        };

        let (weight_q_active_buffer, weight_q_active_memory, weight_q_pos_buffer, weight_q_pos_memory) = create_nda_weight_buffers(&device, weight_q, 3200, 3200)?;
        let (weight_k_active_buffer, weight_k_active_memory, weight_k_pos_buffer, weight_k_pos_memory) = create_nda_weight_buffers(&device, weight_k, 3200, 3200)?;
        let (weight_v_active_buffer, weight_v_active_memory, weight_v_pos_buffer, weight_v_pos_memory) = create_nda_weight_buffers(&device, weight_v, 3200, 3200)?;
        let (weight_o_active_buffer, weight_o_active_memory, weight_o_pos_buffer, weight_o_pos_memory) = create_nda_weight_buffers(&device, weight_o, 3200, 3200)?;
        let (weight_gate_active_buffer, weight_gate_active_memory, weight_gate_pos_buffer, weight_gate_pos_memory) = create_nda_weight_buffers(&device, weight_gate, 3200, 8640)?;
        let (weight_up_active_buffer, weight_up_active_memory, weight_up_pos_buffer, weight_up_pos_memory) = create_nda_weight_buffers(&device, weight_up, 3200, 8640)?;
        let (weight_down_active_buffer, weight_down_active_memory, weight_down_pos_buffer, weight_down_pos_memory) = create_nda_weight_buffers(&device, weight_down, 8640, 3200)?;

        // 6. Descriptor Pool
        let pool_sizes = [
            vk::DescriptorPoolSize::builder().ty(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(40).build(),
        ];
        let pool_info = vk::DescriptorPoolCreateInfo::builder().max_sets(8).pool_sizes(&pool_sizes);
        let desc_pool = unsafe { device.create_descriptor_pool(&pool_info, None)? };
        
        let layouts_vec = vec![desc_set_layout; 8];
        let desc_sets = unsafe { device.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::builder().descriptor_pool(desc_pool).set_layouts(&layouts_vec))? };

        let set_q = desc_sets[0];
        let set_k = desc_sets[1];
        let set_v = desc_sets[2];
        let set_o = desc_sets[3];
        let set_gate = desc_sets[4];
        let set_up = desc_sets[5];
        let set_act = desc_sets[6];
        let set_down = desc_sets[7];

        let bind_set = |device: &Device, set: vk::DescriptorSet, b0: vk::Buffer, s0: vk::DeviceSize, b1: vk::Buffer, s1: vk::DeviceSize, b2: vk::Buffer, s2: vk::DeviceSize, b3: vk::Buffer, s3: vk::DeviceSize, b4: vk::Buffer, s4: vk::DeviceSize| {
            let buffer_infos = [
                vk::DescriptorBufferInfo::builder().buffer(b0).offset(0).range(s0).build(),
                vk::DescriptorBufferInfo::builder().buffer(b1).offset(0).range(s1).build(),
                vk::DescriptorBufferInfo::builder().buffer(b2).offset(0).range(s2).build(),
                vk::DescriptorBufferInfo::builder().buffer(b3).offset(0).range(s3).build(),
                vk::DescriptorBufferInfo::builder().buffer(b4).offset(0).range(s4).build(),
            ];
            let writes = [
                vk::WriteDescriptorSet::builder().dst_set(set).dst_binding(0).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&buffer_infos[0..1]).build(),
                vk::WriteDescriptorSet::builder().dst_set(set).dst_binding(1).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&buffer_infos[1..2]).build(),
                vk::WriteDescriptorSet::builder().dst_set(set).dst_binding(2).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&buffer_infos[2..3]).build(),
                vk::WriteDescriptorSet::builder().dst_set(set).dst_binding(3).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&buffer_infos[3..4]).build(),
                vk::WriteDescriptorSet::builder().dst_set(set).dst_binding(4).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&buffer_infos[4..5]).build(),
            ];
            unsafe { device.update_descriptor_sets(&writes, &[]) };
        };

        bind_set(&device, set_q, inputs_3200_active_buffer, 400, inputs_3200_pos_buffer, 400, weight_q_active_buffer, 1280000, weight_q_pos_buffer, 1280000, out_3200_q_buffer, 3200 * 4);
        bind_set(&device, set_k, inputs_3200_active_buffer, 400, inputs_3200_pos_buffer, 400, weight_k_active_buffer, 1280000, weight_k_pos_buffer, 1280000, out_3200_k_buffer, 3200 * 4);
        bind_set(&device, set_v, inputs_3200_active_buffer, 400, inputs_3200_pos_buffer, 400, weight_v_active_buffer, 1280000, weight_v_pos_buffer, 1280000, out_3200_v_buffer, 3200 * 4);
        bind_set(&device, set_o, inputs_3200_active_buffer, 400, inputs_3200_pos_buffer, 400, weight_o_active_buffer, 1280000, weight_o_pos_buffer, 1280000, out_3200_o_buffer, 3200 * 4);
        bind_set(&device, set_gate, inputs_3200_active_buffer, 400, inputs_3200_pos_buffer, 400, weight_gate_active_buffer, 3456000, weight_gate_pos_buffer, 3456000, out_8640_gate_buffer, 8640 * 4);
        bind_set(&device, set_up, inputs_3200_active_buffer, 400, inputs_3200_pos_buffer, 400, weight_up_active_buffer, 3456000, weight_up_pos_buffer, 3456000, out_8640_up_buffer, 8640 * 4);
        
        bind_set(&device, set_act, out_8640_gate_buffer, 8640 * 4, out_8640_up_buffer, 8640 * 4, inputs_8640_active_buffer, 1080, inputs_8640_pos_buffer, 1080, out_3200_down_buffer, 3200 * 4);
        
        bind_set(&device, set_down, inputs_8640_active_buffer, 1080, inputs_8640_pos_buffer, 1080, weight_down_active_buffer, 3456000, weight_down_pos_buffer, 3456000, out_3200_down_buffer, 3200 * 4);

        // 7. Command Buffer
        let command_pool_info = vk::CommandPoolCreateInfo::builder().queue_family_index(driver.queue_family_index).flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = unsafe { device.create_command_pool(&command_pool_info, None)? };
        let command_buffers = unsafe { device.allocate_command_buffers(&vk::CommandBufferAllocateInfo::builder().command_pool(command_pool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(1))? };
        let command_buffer = command_buffers[0];

        let begin_info = vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::SIMULTANEOUS_USE);
        unsafe {
            device.begin_command_buffer(command_buffer, &begin_info)?;
            
            let dispatch_nda = |cmd: vk::CommandBuffer, pipe: vk::Pipeline, layout: vk::PipelineLayout, set: vk::DescriptorSet, k: u32, n: u32| {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, pipe);
                device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::COMPUTE, layout, 0, &[set], &[]);
                let params = [k, n];
                let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
                device.cmd_push_constants(cmd, layout, vk::ShaderStageFlags::COMPUTE, 0, params_bytes);
                let workgroups = (n + 255) / 256;
                device.cmd_dispatch(cmd, workgroups, 1, 1);
            };

            dispatch_nda(command_buffer, pipeline_nda, pipeline_layout, set_q, 3200, 3200);
            dispatch_nda(command_buffer, pipeline_nda, pipeline_layout, set_k, 3200, 3200);
            dispatch_nda(command_buffer, pipeline_nda, pipeline_layout, set_v, 3200, 3200);
            dispatch_nda(command_buffer, pipeline_nda, pipeline_layout, set_o, 3200, 3200);
            dispatch_nda(command_buffer, pipeline_nda, pipeline_layout, set_gate, 3200, 8640);
            dispatch_nda(command_buffer, pipeline_nda, pipeline_layout, set_up, 3200, 8640);

            // Barrier before Activation
            let barrier_act = vk::BufferMemoryBarrier::builder()
                .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .buffer(out_8640_gate_buffer)
                .offset(0)
                .size(vk::WHOLE_SIZE)
                .build();
            let barrier_up = vk::BufferMemoryBarrier::builder()
                .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .buffer(out_8640_up_buffer)
                .offset(0)
                .size(vk::WHOLE_SIZE)
                .build();
            device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[barrier_act, barrier_up],
                &[],
            );

            // Dispatch Activation
            device.cmd_bind_pipeline(command_buffer, vk::PipelineBindPoint::COMPUTE, pipeline_act);
            device.cmd_bind_descriptor_sets(command_buffer, vk::PipelineBindPoint::COMPUTE, pipeline_layout, 0, &[set_act], &[]);
            let act_params = [8640u32, 0u32];
            let act_params_bytes = std::slice::from_raw_parts(act_params.as_ptr() as *const u8, 8);
            device.cmd_push_constants(command_buffer, pipeline_layout, vk::ShaderStageFlags::COMPUTE, 0, act_params_bytes);
            device.cmd_dispatch(command_buffer, (8640 + 63) / 64, 1, 1);

            // Barrier before Down projection
            let barrier_down_act = vk::BufferMemoryBarrier::builder()
                .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .buffer(inputs_8640_active_buffer)
                .offset(0)
                .size(vk::WHOLE_SIZE)
                .build();
            let barrier_down_pos = vk::BufferMemoryBarrier::builder()
                .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .buffer(inputs_8640_pos_buffer)
                .offset(0)
                .size(vk::WHOLE_SIZE)
                .build();
            device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[barrier_down_act, barrier_down_pos],
                &[],
            );

            // Dispatch Down GEMV
            dispatch_nda(command_buffer, pipeline_nda, pipeline_layout, set_down, 8640, 3200);

            device.end_command_buffer(command_buffer)?;
        }

        let fence = unsafe { device.create_fence(&vk::FenceCreateInfo::builder(), None)? };

        Ok(Self {
            device,
            queue,
            shader_nda,
            shader_act,
            desc_set_layout,
            pipeline_layout,
            pipeline_nda,
            pipeline_act,
            inputs_3200_active_buffer,
            inputs_3200_active_memory,
            inputs_3200_active_ptr,
            inputs_3200_pos_buffer,
            inputs_3200_pos_memory,
            inputs_3200_pos_ptr,
            out_3200_down_buffer,
            out_3200_down_memory,
            out_3200_down_ptr,
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
            desc_sets,
            command_pool,
            command_buffer,
            fence,
        })
    }

    pub fn run(
        &self,
        input_active: &[u8],
        input_pos: &[u8],
        output_floats: &mut [f32],
    ) -> Result<f64, Box<dyn std::error::Error>> {
        unsafe {
            std::ptr::copy_nonoverlapping(input_active.as_ptr(), self.inputs_3200_active_ptr as *mut u8, input_active.len());
            std::ptr::copy_nonoverlapping(input_pos.as_ptr(), self.inputs_3200_pos_ptr as *mut u8, input_pos.len());
        }

        let start = Instant::now();
        unsafe {
            self.device.reset_fences(&[self.fence])?;
            self.device.queue_submit(self.queue, &[vk::SubmitInfo::builder().command_buffers(&[self.command_buffer]).build()], self.fence)?;
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        let duration_us = start.elapsed().as_micros() as f64;

        unsafe {
            std::ptr::copy_nonoverlapping(self.out_3200_down_ptr as *const f32, output_floats.as_mut_ptr(), output_floats.len());
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
            
            let destroy_buffer = |device: &Device, buffer: vk::Buffer, memory: vk::DeviceMemory, mapped: bool| {
                if mapped {
                    device.unmap_memory(memory);
                }
                device.free_memory(memory, None);
                device.destroy_buffer(buffer, None);
            };

            destroy_buffer(&self.device, self.inputs_3200_active_buffer, self.inputs_3200_active_memory, true);
            destroy_buffer(&self.device, self.inputs_3200_pos_buffer, self.inputs_3200_pos_memory, true);
            destroy_buffer(&self.device, self.out_3200_down_buffer, self.out_3200_down_memory, true);
            destroy_buffer(&self.device, self.out_3200_q_buffer, self.out_3200_q_memory, false);
            destroy_buffer(&self.device, self.out_3200_k_buffer, self.out_3200_k_memory, false);
            destroy_buffer(&self.device, self.out_3200_v_buffer, self.out_3200_v_memory, false);
            destroy_buffer(&self.device, self.out_3200_o_buffer, self.out_3200_o_memory, false);
            destroy_buffer(&self.device, self.out_8640_gate_buffer, self.out_8640_gate_memory, false);
            destroy_buffer(&self.device, self.out_8640_up_buffer, self.out_8640_up_memory, false);
            destroy_buffer(&self.device, self.inputs_8640_active_buffer, self.inputs_8640_active_memory, false);
            destroy_buffer(&self.device, self.inputs_8640_pos_buffer, self.inputs_8640_pos_memory, false);

            destroy_buffer(&self.device, self.weight_q_active_buffer, self.weight_q_active_memory, false);
            destroy_buffer(&self.device, self.weight_q_pos_buffer, self.weight_q_pos_memory, false);
            destroy_buffer(&self.device, self.weight_k_active_buffer, self.weight_k_active_memory, false);
            destroy_buffer(&self.device, self.weight_k_pos_buffer, self.weight_k_pos_memory, false);
            destroy_buffer(&self.device, self.weight_v_active_buffer, self.weight_v_active_memory, false);
            destroy_buffer(&self.device, self.weight_v_pos_buffer, self.weight_v_pos_memory, false);
            destroy_buffer(&self.device, self.weight_o_active_buffer, self.weight_o_active_memory, false);
            destroy_buffer(&self.device, self.weight_o_pos_buffer, self.weight_o_pos_memory, false);
            destroy_buffer(&self.device, self.weight_gate_active_buffer, self.weight_gate_active_memory, false);
            destroy_buffer(&self.device, self.weight_gate_pos_buffer, self.weight_gate_pos_memory, false);
            destroy_buffer(&self.device, self.weight_up_active_buffer, self.weight_up_active_memory, false);
            destroy_buffer(&self.device, self.weight_up_pos_buffer, self.weight_up_pos_memory, false);
            destroy_buffer(&self.device, self.weight_down_active_buffer, self.weight_down_active_memory, false);
            destroy_buffer(&self.device, self.weight_down_pos_buffer, self.weight_down_pos_memory, false);

            self.device.destroy_pipeline(self.pipeline_nda, None);
            self.device.destroy_pipeline(self.pipeline_act, None);
            self.device.destroy_pipeline_layout(self.pipeline_layout, None);
            self.device.destroy_descriptor_set_layout(self.desc_set_layout, None);
            
            self.device.destroy_shader_module(self.shader_nda, None);
            self.device.destroy_shader_module(self.shader_act, None);
        }
    }
}

// ─── GPU-Native Model Execution Pipeline ────────────────────────────────────

pub struct LayerGpuGemvs<'a> {
    pub q_proj_gpu: &'a Option<VulkanNdaGemv>,
    pub k_proj_gpu: &'a Option<VulkanNdaGemv>,
    pub v_proj_gpu: &'a Option<VulkanNdaGemv>,
    pub o_proj_gpu: &'a Option<VulkanNdaGemv>,
    pub gate_proj_gpu: &'a Option<VulkanNdaGemv>,
    pub up_proj_gpu: &'a Option<VulkanNdaGemv>,
    pub down_proj_gpu: &'a Option<VulkanNdaGemv>,
}

pub struct VulkanModelPipeline {
    pub device: Device,
    pub queue: vk::Queue,
    
    // Shader Modules
    pub shader_rms_norm: vk::ShaderModule,
    pub shader_rope: vk::ShaderModule,
    pub shader_kv_write: vk::ShaderModule,
    pub shader_attn_softmax: vk::ShaderModule,
    pub shader_swiglu: vk::ShaderModule,
    pub shader_residual_add: vk::ShaderModule,
    pub shader_bias_add: vk::ShaderModule,
    
    // Layouts
    pub layout_rms_norm: vk::PipelineLayout,
    pub layout_rope: vk::PipelineLayout,
    pub layout_kv_write: vk::PipelineLayout,
    pub layout_attn_softmax: vk::PipelineLayout,
    pub layout_swiglu: vk::PipelineLayout,
    pub layout_residual_add: vk::PipelineLayout,
    pub layout_bias_add: vk::PipelineLayout,
    
    pub desc_layout_2: vk::DescriptorSetLayout,
    pub desc_layout_3: vk::DescriptorSetLayout,
    
    // Pipelines
    pub pipeline_rms_norm: vk::Pipeline,
    pub pipeline_rope: vk::Pipeline,
    pub pipeline_kv_write: vk::Pipeline,
    pub pipeline_attn_softmax: vk::Pipeline,
    pub pipeline_swiglu: vk::Pipeline,
    pub pipeline_residual_add: vk::Pipeline,
    pub pipeline_bias_add: vk::Pipeline,
    
    // Intermediate Buffers & Memory
    pub x_residual_buffer: vk::Buffer,
    pub x_residual_memory: vk::DeviceMemory,
    pub x_residual_ptr: *mut std::ffi::c_void,
    
    pub attn_out_buffer: vk::Buffer,
    pub attn_out_memory: vk::DeviceMemory,
    pub gated_buffer: vk::Buffer,
    pub gated_memory: vk::DeviceMemory,
    
    // Persistent Layer Buffers
    pub layer_attn_norms: Vec<(vk::Buffer, vk::DeviceMemory)>,
    pub layer_ffn_norms: Vec<(vk::Buffer, vk::DeviceMemory)>,
    pub layer_q_biases: Vec<Option<(vk::Buffer, vk::DeviceMemory)>>,
    pub layer_k_biases: Vec<Option<(vk::Buffer, vk::DeviceMemory)>>,
    pub layer_v_biases: Vec<Option<(vk::Buffer, vk::DeviceMemory)>>,
    pub final_norm_buf: vk::Buffer,
    pub final_norm_mem: vk::DeviceMemory,
    
    // KV Cache Buffers
    pub layer_kv_caches: Vec<(vk::Buffer, vk::DeviceMemory)>,
    
    // Descriptors & Command Execution
    pub desc_pool: vk::DescriptorPool,
    pub command_pool: vk::CommandPool,
    pub command_buffer: vk::CommandBuffer,
    pub fence: vk::Fence,

    // Pre-allocated Descriptor Sets for each layer's operations
    pub desc_sets_rms_norm_attn: Vec<vk::DescriptorSet>,
    pub desc_sets_rms_norm_ffn: Vec<vk::DescriptorSet>,
    pub desc_sets_bias_q: Vec<Option<vk::DescriptorSet>>,
    pub desc_sets_bias_k: Vec<Option<vk::DescriptorSet>>,
    pub desc_sets_bias_v: Vec<Option<vk::DescriptorSet>>,
    pub desc_sets_rope: Vec<vk::DescriptorSet>,
    pub desc_sets_kv_write: Vec<vk::DescriptorSet>,
    pub desc_sets_attn_softmax: Vec<vk::DescriptorSet>,
    pub desc_sets_residual_add_attn: Vec<vk::DescriptorSet>,
    pub desc_sets_swiglu: Vec<vk::DescriptorSet>,
    pub desc_sets_residual_add_ffn: Vec<vk::DescriptorSet>,
    pub desc_set_final_norm: vk::DescriptorSet,
}

impl VulkanModelPipeline {
    pub fn new(
        driver: &VulkanDriver,
        n_layers: usize,
        hidden_size: usize,
        ffn_size: usize,
        _n_heads: usize,
        n_kv_heads: usize,
        head_dim: usize,
        max_seq_len: usize,
        attn_norm_weights: &[&[f32]],
        ffn_norm_weights: &[&[f32]],
        q_biases: &[Option<&[f32]>],
        k_biases: &[Option<&[f32]>],
        v_biases: &[Option<&[f32]>],
        final_norm_weight: &[f32],
        layers_gpu: &[&LayerGpuGemvs],
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let device = driver.device.clone();
        let physical_device = driver.physical_device;
        let instance = driver.instance.clone();
        let queue = driver.compute_queue;

        // 1. Create Shader Modules
        let shader_rms_norm = create_shader_module(&device, crate::compiler::shaders::RMS_NORM_SPV)?;
        let shader_rope = create_shader_module(&device, crate::compiler::shaders::ROPE_SPV)?;
        let shader_kv_write = create_shader_module(&device, crate::compiler::shaders::KV_WRITE_SPV)?;
        let shader_attn_softmax = create_shader_module(&device, crate::compiler::shaders::ATTN_SOFTMAX_SPV)?;
        let shader_swiglu = create_shader_module(&device, crate::compiler::shaders::SWIGLU_SPV)?;
        let shader_residual_add = create_shader_module(&device, crate::compiler::shaders::RESIDUAL_ADD_SPV)?;
        let shader_bias_add = create_shader_module(&device, crate::compiler::shaders::BIAS_ADD_SPV)?;

        // 2. Create Descriptor Set Layouts
        let bindings_2 = [
            vk::DescriptorSetLayoutBinding::builder().binding(0).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
            vk::DescriptorSetLayoutBinding::builder().binding(1).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
        ];
        let desc_layout_2 = create_desc_layout(&device, &bindings_2)?;

        let bindings_3 = [
            vk::DescriptorSetLayoutBinding::builder().binding(0).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
            vk::DescriptorSetLayoutBinding::builder().binding(1).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
            vk::DescriptorSetLayoutBinding::builder().binding(2).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE).build(),
        ];
        let desc_layout_3 = create_desc_layout(&device, &bindings_3)?;

        // 3. Create Pipeline Layouts
        let layout_rms_norm = create_pipeline_layout(&device, desc_layout_2, 8)?; // H, eps
        let layout_rope = create_pipeline_layout(&device, desc_layout_2, 20)?; // pos, head_dim, n_heads, n_kv_heads, theta
        let layout_kv_write = create_pipeline_layout(&device, desc_layout_3, 12)?; // pos, kv_dim, max_seq
        let layout_attn_softmax = create_pipeline_layout(&device, desc_layout_3, 24)?; // pos, head_dim, n_heads, n_kv_heads, max_seq, scale
        let layout_swiglu = create_pipeline_layout(&device, desc_layout_3, 4)?; // size
        let layout_residual_add = create_pipeline_layout(&device, desc_layout_2, 4)?; // size
        let layout_bias_add = create_pipeline_layout(&device, desc_layout_2, 4)?; // size

        // 4. Create Pipelines
        let pipeline_rms_norm = create_compute_pipeline(&device, shader_rms_norm, layout_rms_norm)?;
        let pipeline_rope = create_compute_pipeline(&device, shader_rope, layout_rope)?;
        let pipeline_kv_write = create_compute_pipeline(&device, shader_kv_write, layout_kv_write)?;
        let pipeline_attn_softmax = create_compute_pipeline(&device, shader_attn_softmax, layout_attn_softmax)?;
        let pipeline_swiglu = create_compute_pipeline(&device, shader_swiglu, layout_swiglu)?;
        let pipeline_residual_add = create_compute_pipeline(&device, shader_residual_add, layout_residual_add)?;
        let pipeline_bias_add = create_compute_pipeline(&device, shader_bias_add, layout_bias_add)?;

        // 5. Allocate Intermediate Buffers
        let (x_residual_buffer, x_residual_memory, x_residual_ptr) = create_coherent_buffer(&device, &instance, physical_device, (hidden_size * 4) as vk::DeviceSize, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (attn_out_buffer, attn_out_memory) = create_uninitialized_device_local_buffer(&device, &instance, physical_device, (hidden_size * 4) as vk::DeviceSize, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let (gated_buffer, gated_memory) = create_uninitialized_device_local_buffer(&device, &instance, physical_device, (ffn_size * 4) as vk::DeviceSize, vk::BufferUsageFlags::STORAGE_BUFFER)?;

        // 6. Allocate and Upload Persistent Layer Buffers
        let mut layer_attn_norms = Vec::with_capacity(n_layers);
        let mut layer_ffn_norms = Vec::with_capacity(n_layers);
        let mut layer_q_biases = Vec::with_capacity(n_layers);
        let mut layer_k_biases = Vec::with_capacity(n_layers);
        let mut layer_v_biases = Vec::with_capacity(n_layers);
        let mut layer_kv_caches = Vec::with_capacity(n_layers);

        let kv_dim = n_kv_heads * head_dim;

        for i in 0..n_layers {
            let bytes_attn = unsafe { std::slice::from_raw_parts(attn_norm_weights[i].as_ptr() as *const u8, attn_norm_weights[i].len() * 4) };
            let (attn_buf, attn_mem) = create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, bytes_attn.len() as vk::DeviceSize, bytes_attn)?;
            layer_attn_norms.push((attn_buf, attn_mem));

            let bytes_ffn = unsafe { std::slice::from_raw_parts(ffn_norm_weights[i].as_ptr() as *const u8, ffn_norm_weights[i].len() * 4) };
            let (ffn_buf, ffn_mem) = create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, bytes_ffn.len() as vk::DeviceSize, bytes_ffn)?;
            layer_ffn_norms.push((ffn_buf, ffn_mem));

            if let Some(qb) = q_biases[i] {
                let bytes = unsafe { std::slice::from_raw_parts(qb.as_ptr() as *const u8, qb.len() * 4) };
                let (buf, mem) = create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, bytes.len() as vk::DeviceSize, bytes)?;
                layer_q_biases.push(Some((buf, mem)));
            } else {
                layer_q_biases.push(None);
            }

            if let Some(kb) = k_biases[i] {
                let bytes = unsafe { std::slice::from_raw_parts(kb.as_ptr() as *const u8, kb.len() * 4) };
                let (buf, mem) = create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, bytes.len() as vk::DeviceSize, bytes)?;
                layer_k_biases.push(Some((buf, mem)));
            } else {
                layer_k_biases.push(None);
            }

            if let Some(vb) = v_biases[i] {
                let bytes = unsafe { std::slice::from_raw_parts(vb.as_ptr() as *const u8, vb.len() * 4) };
                let (buf, mem) = create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, bytes.len() as vk::DeviceSize, bytes)?;
                layer_v_biases.push(Some((buf, mem)));
            } else {
                layer_v_biases.push(None);
            }

            let cache_size = 2 * max_seq_len * kv_dim * 4;
            let (cache_buf, cache_mem) = create_uninitialized_device_local_buffer(&device, &instance, physical_device, cache_size as vk::DeviceSize, vk::BufferUsageFlags::STORAGE_BUFFER)?;
            layer_kv_caches.push((cache_buf, cache_mem));
        }

        let bytes_final = unsafe { std::slice::from_raw_parts(final_norm_weight.as_ptr() as *const u8, final_norm_weight.len() * 4) };
        let (final_norm_buf, final_norm_mem) = create_device_local_buffer(&device, &instance, physical_device, queue, driver.queue_family_index, bytes_final.len() as vk::DeviceSize, bytes_final)?;

        // 7. Descriptor Pool & Sets allocation
        let mut total_sets_2 = n_layers * 5 + 1; // RMSNorm attn, RMSNorm ffn, RoPE, Residual x2, Final Norm
        for i in 0..n_layers {
            if q_biases[i].is_some() { total_sets_2 += 1; }
            if k_biases[i].is_some() { total_sets_2 += 1; }
            if v_biases[i].is_some() { total_sets_2 += 1; }
        }
        let total_sets_3 = n_layers * 3; // KV Write, Attn Softmax, SwiGLU
        let total_sets = total_sets_2 + total_sets_3;

        let pool_sizes = [
            vk::DescriptorPoolSize::builder().ty(vk::DescriptorType::STORAGE_BUFFER).descriptor_count((total_sets * 3) as u32).build(),
        ];
        let pool_info = vk::DescriptorPoolCreateInfo::builder()
            .pool_sizes(&pool_sizes)
            .max_sets(total_sets as u32);
        let desc_pool = unsafe { device.create_descriptor_pool(&pool_info, None)? };

        // Allocate Descriptor Sets
        let layouts_2 = vec![desc_layout_2; total_sets_2];
        let alloc_info_2 = vk::DescriptorSetAllocateInfo::builder()
            .descriptor_pool(desc_pool)
            .set_layouts(&layouts_2);
        let sets_2 = unsafe { device.allocate_descriptor_sets(&alloc_info_2)? };

        let layouts_3 = vec![desc_layout_3; total_sets_3];
        let alloc_info_3 = vk::DescriptorSetAllocateInfo::builder()
            .descriptor_pool(desc_pool)
            .set_layouts(&layouts_3);
        let sets_3 = unsafe { device.allocate_descriptor_sets(&alloc_info_3)? };

        let mut sets_2_iter = sets_2.into_iter();
        let mut sets_3_iter = sets_3.into_iter();

        let mut desc_sets_rms_norm_attn = Vec::with_capacity(n_layers);
        let mut desc_sets_rms_norm_ffn = Vec::with_capacity(n_layers);
        let mut desc_sets_bias_q = Vec::with_capacity(n_layers);
        let mut desc_sets_bias_k = Vec::with_capacity(n_layers);
        let mut desc_sets_bias_v = Vec::with_capacity(n_layers);
        let mut desc_sets_rope = Vec::with_capacity(n_layers);
        let mut desc_sets_kv_write = Vec::with_capacity(n_layers);
        let mut desc_sets_attn_softmax = Vec::with_capacity(n_layers);
        let mut desc_sets_residual_add_attn = Vec::with_capacity(n_layers);
        let mut desc_sets_swiglu = Vec::with_capacity(n_layers);
        let mut desc_sets_residual_add_ffn = Vec::with_capacity(n_layers);

        // Pre-allocate buffer infos to avoid reallocation
        let mut buffer_infos = Vec::with_capacity(total_sets * 3);
        let mut writes = Vec::new();

        // Helper to push descriptor write
        let push_write_2 = |set: vk::DescriptorSet, b0: vk::Buffer, b1: vk::Buffer, infos: &mut Vec<vk::DescriptorBufferInfo>, writes_list: &mut Vec<vk::WriteDescriptorSet>| {
            let idx = infos.len();
            infos.push(vk::DescriptorBufferInfo::builder().buffer(b0).offset(0).range(vk::WHOLE_SIZE).build());
            infos.push(vk::DescriptorBufferInfo::builder().buffer(b1).offset(0).range(vk::WHOLE_SIZE).build());

            writes_list.push(vk::WriteDescriptorSet::builder().dst_set(set).dst_binding(0).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&infos[idx..idx+1]).build());
            writes_list.push(vk::WriteDescriptorSet::builder().dst_set(set).dst_binding(1).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&infos[idx+1..idx+2]).build());
        };

        let push_write_3 = |set: vk::DescriptorSet, b0: vk::Buffer, b1: vk::Buffer, b2: vk::Buffer, infos: &mut Vec<vk::DescriptorBufferInfo>, writes_list: &mut Vec<vk::WriteDescriptorSet>| {
            let idx = infos.len();
            infos.push(vk::DescriptorBufferInfo::builder().buffer(b0).offset(0).range(vk::WHOLE_SIZE).build());
            infos.push(vk::DescriptorBufferInfo::builder().buffer(b1).offset(0).range(vk::WHOLE_SIZE).build());
            infos.push(vk::DescriptorBufferInfo::builder().buffer(b2).offset(0).range(vk::WHOLE_SIZE).build());

            writes_list.push(vk::WriteDescriptorSet::builder().dst_set(set).dst_binding(0).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&infos[idx..idx+1]).build());
            writes_list.push(vk::WriteDescriptorSet::builder().dst_set(set).dst_binding(1).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&infos[idx+1..idx+2]).build());
            writes_list.push(vk::WriteDescriptorSet::builder().dst_set(set).dst_binding(2).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&infos[idx+2..idx+3]).build());
        };

        for i in 0..n_layers {
            let lg = layers_gpu[i];
            let q_buf = lg.q_proj_gpu.as_ref().map(|g| g.output_buffer).unwrap_or(vk::Buffer::null());
            let k_buf = lg.k_proj_gpu.as_ref().map(|g| g.output_buffer).unwrap_or(vk::Buffer::null());
            let v_buf = lg.v_proj_gpu.as_ref().map(|g| g.output_buffer).unwrap_or(vk::Buffer::null());
            let o_buf = lg.o_proj_gpu.as_ref().map(|g| g.output_buffer).unwrap_or(vk::Buffer::null());
            let gate_buf = lg.gate_proj_gpu.as_ref().map(|g| g.output_buffer).unwrap_or(vk::Buffer::null());
            let up_buf = lg.up_proj_gpu.as_ref().map(|g| g.output_buffer).unwrap_or(vk::Buffer::null());
            let down_buf = lg.down_proj_gpu.as_ref().map(|g| g.output_buffer).unwrap_or(vk::Buffer::null());

            // 1. RMSNorm attn
            let set_rms_attn = sets_2_iter.next().unwrap();
            push_write_2(set_rms_attn, driver.shared_input_buffer, layer_attn_norms[i].0, &mut buffer_infos, &mut writes);
            desc_sets_rms_norm_attn.push(set_rms_attn);

            // 2. Bias Q
            if let Some(ref bias) = layer_q_biases[i] {
                let set = sets_2_iter.next().unwrap();
                push_write_2(set, q_buf, bias.0, &mut buffer_infos, &mut writes);
                desc_sets_bias_q.push(Some(set));
            } else {
                desc_sets_bias_q.push(None);
            }

            // 3. Bias K
            if let Some(ref bias) = layer_k_biases[i] {
                let set = sets_2_iter.next().unwrap();
                push_write_2(set, k_buf, bias.0, &mut buffer_infos, &mut writes);
                desc_sets_bias_k.push(Some(set));
            } else {
                desc_sets_bias_k.push(None);
            }

            // 4. Bias V
            if let Some(ref bias) = layer_v_biases[i] {
                let set = sets_2_iter.next().unwrap();
                push_write_2(set, v_buf, bias.0, &mut buffer_infos, &mut writes);
                desc_sets_bias_v.push(Some(set));
            } else {
                desc_sets_bias_v.push(None);
            }

            // 5. RoPE
            let set_rope = sets_2_iter.next().unwrap();
            push_write_2(set_rope, q_buf, k_buf, &mut buffer_infos, &mut writes);
            desc_sets_rope.push(set_rope);

            // 6. KV Write
            let set_kv_write = sets_3_iter.next().unwrap();
            push_write_3(set_kv_write, k_buf, v_buf, layer_kv_caches[i].0, &mut buffer_infos, &mut writes);
            desc_sets_kv_write.push(set_kv_write);

            // 7. Attn Softmax
            let set_attn = sets_3_iter.next().unwrap();
            push_write_3(set_attn, q_buf, layer_kv_caches[i].0, attn_out_buffer, &mut buffer_infos, &mut writes);
            desc_sets_attn_softmax.push(set_attn);

            // 8. Residual Add attn
            let set_res_attn = sets_2_iter.next().unwrap();
            push_write_2(set_res_attn, x_residual_buffer, o_buf, &mut buffer_infos, &mut writes);
            desc_sets_residual_add_attn.push(set_res_attn);

            // 9. RMSNorm ffn
            let set_rms_ffn = sets_2_iter.next().unwrap();
            push_write_2(set_rms_ffn, driver.shared_input_buffer, layer_ffn_norms[i].0, &mut buffer_infos, &mut writes);
            desc_sets_rms_norm_ffn.push(set_rms_ffn);

            // 10. SwiGLU
            let set_swiglu = sets_3_iter.next().unwrap();
            push_write_3(set_swiglu, gate_buf, up_buf, gated_buffer, &mut buffer_infos, &mut writes);
            desc_sets_swiglu.push(set_swiglu);

            // 11. Residual Add ffn
            let set_res_ffn = sets_2_iter.next().unwrap();
            push_write_2(set_res_ffn, x_residual_buffer, down_buf, &mut buffer_infos, &mut writes);
            desc_sets_residual_add_ffn.push(set_res_ffn);
        }

        // Final Norm
        let desc_set_final_norm = sets_2_iter.next().unwrap();
        push_write_2(desc_set_final_norm, x_residual_buffer, final_norm_buf, &mut buffer_infos, &mut writes);

        unsafe { device.update_descriptor_sets(&writes, &[]) };

        // 8. Command Buffer & Execution Setup
        let command_pool_info = vk::CommandPoolCreateInfo::builder().queue_family_index(driver.queue_family_index).flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = unsafe { device.create_command_pool(&command_pool_info, None)? };
        let command_buffers = unsafe { device.allocate_command_buffers(&vk::CommandBufferAllocateInfo::builder().command_pool(command_pool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(1))? };
        let command_buffer = command_buffers[0];

        let fence_info = vk::FenceCreateInfo::builder();
        let fence = unsafe { device.create_fence(&fence_info, None)? };

        Ok(Self {
            device,
            queue,
            shader_rms_norm,
            shader_rope,
            shader_kv_write,
            shader_attn_softmax,
            shader_swiglu,
            shader_residual_add,
            shader_bias_add,
            layout_rms_norm,
            layout_rope,
            layout_kv_write,
            layout_attn_softmax,
            layout_swiglu,
            layout_residual_add,
            layout_bias_add,
            desc_layout_2,
            desc_layout_3,
            pipeline_rms_norm,
            pipeline_rope,
            pipeline_kv_write,
            pipeline_attn_softmax,
            pipeline_swiglu,
            pipeline_residual_add,
            pipeline_bias_add,
            x_residual_buffer,
            x_residual_memory,
            x_residual_ptr,
            attn_out_buffer,
            attn_out_memory,
            gated_buffer,
            gated_memory,
            layer_attn_norms,
            layer_ffn_norms,
            layer_q_biases,
            layer_k_biases,
            layer_v_biases,
            final_norm_buf,
            final_norm_mem,
            layer_kv_caches,
            desc_pool,
            command_pool,
            command_buffer,
            fence,
            desc_sets_rms_norm_attn,
            desc_sets_rms_norm_ffn,
            desc_sets_bias_q,
            desc_sets_bias_k,
            desc_sets_bias_v,
            desc_sets_rope,
            desc_sets_kv_write,
            desc_sets_attn_softmax,
            desc_sets_residual_add_attn,
            desc_sets_swiglu,
            desc_sets_residual_add_ffn,
            desc_set_final_norm,
        })
    }

    pub fn record_and_execute_token(
        &self,
        driver: &VulkanDriver,
        n_layers: usize,
        hidden_size: usize,
        ffn_size: usize,
        n_heads: usize,
        n_kv_heads: usize,
        head_dim: usize,
        max_seq_len: usize,
        rope_theta: f32,
        scale: f32,
        pos: u32,
        layers_gpu: &[&LayerGpuGemvs],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let device = &self.device;
        let cmd = self.command_buffer;

        // 1. Begin Command Buffer
        let begin_info = vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        unsafe {
            device.begin_command_buffer(cmd, &begin_info)?;
        }

        let kv_dim = n_kv_heads * head_dim;

        // 2. Record layers
        for i in 0..n_layers {
            let lg = layers_gpu[i];
            
            // 2.1 Copy x_residual_buffer to driver.shared_input_buffer
            cmd_compute_to_transfer_barrier(device, cmd);
            unsafe {
                device.cmd_copy_buffer(cmd, self.x_residual_buffer, driver.shared_input_buffer, &[vk::BufferCopy::builder().size((hidden_size * 4) as vk::DeviceSize).build()]);
            }
            cmd_transfer_to_compute_barrier(device, cmd);

            // 2.2 Attention pre-norm (in-place on driver.shared_input_buffer)
            unsafe {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, self.pipeline_rms_norm);
                device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::COMPUTE, self.layout_rms_norm, 0, &[self.desc_sets_rms_norm_attn[i]], &[]);
                
                let params = [hidden_size as u32, 1e-6f32.to_bits()]; // eps = 1e-6
                let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
                device.cmd_push_constants(cmd, self.layout_rms_norm, vk::ShaderStageFlags::COMPUTE, 0, params_bytes);
                device.cmd_dispatch(cmd, 1, 1, 1);
            }
            cmd_compute_barrier(device, cmd);

            // 2.3 Dispatch QKV projections (reads from driver.shared_input_buffer, write to outputs)
            if let Some(ref g) = lg.q_proj_gpu { g.record_dispatch(cmd); }
            if let Some(ref g) = lg.k_proj_gpu { g.record_dispatch(cmd); }
            if let Some(ref g) = lg.v_proj_gpu { g.record_dispatch(cmd); }
            cmd_compute_barrier(device, cmd);

            // 2.4 Add QKV biases if present
            if let Some(ref bias_q_set) = self.desc_sets_bias_q[i] {
                unsafe {
                    device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, self.pipeline_bias_add);
                    device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::COMPUTE, self.layout_bias_add, 0, &[*bias_q_set], &[]);
                    let params = [hidden_size as u32];
                    let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 4);
                    device.cmd_push_constants(cmd, self.layout_bias_add, vk::ShaderStageFlags::COMPUTE, 0, params_bytes);
                    device.cmd_dispatch(cmd, (hidden_size as u32 + 255) / 256, 1, 1);
                }
            }
            if let Some(ref bias_k_set) = self.desc_sets_bias_k[i] {
                unsafe {
                    device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, self.pipeline_bias_add);
                    device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::COMPUTE, self.layout_bias_add, 0, &[*bias_k_set], &[]);
                    let params = [kv_dim as u32];
                    let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 4);
                    device.cmd_push_constants(cmd, self.layout_bias_add, vk::ShaderStageFlags::COMPUTE, 0, params_bytes);
                    device.cmd_dispatch(cmd, (kv_dim as u32 + 255) / 256, 1, 1);
                }
            }
            if let Some(ref bias_v_set) = self.desc_sets_bias_v[i] {
                unsafe {
                    device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, self.pipeline_bias_add);
                    device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::COMPUTE, self.layout_bias_add, 0, &[*bias_v_set], &[]);
                    let params = [kv_dim as u32];
                    let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 4);
                    device.cmd_push_constants(cmd, self.layout_bias_add, vk::ShaderStageFlags::COMPUTE, 0, params_bytes);
                    device.cmd_dispatch(cmd, (kv_dim as u32 + 255) / 256, 1, 1);
                }
            }
            if self.desc_sets_bias_q[i].is_some() || self.desc_sets_bias_k[i].is_some() || self.desc_sets_bias_v[i].is_some() {
                cmd_compute_barrier(device, cmd);
            }

            // 2.5 RoPE (in-place on Q/K output buffers)
            unsafe {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, self.pipeline_rope);
                device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::COMPUTE, self.layout_rope, 0, &[self.desc_sets_rope[i]], &[]);
                
                let params = [pos, head_dim as u32, n_heads as u32, n_kv_heads as u32, rope_theta.to_bits()];
                let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 20);
                device.cmd_push_constants(cmd, self.layout_rope, vk::ShaderStageFlags::COMPUTE, 0, params_bytes);
                
                let total_q_pairs = n_heads * (head_dim / 2);
                device.cmd_dispatch(cmd, (total_q_pairs as u32 + 63) / 64, 1, 1);
            }
            cmd_compute_barrier(device, cmd);

            // 2.6 KV Write (write K/V to cache)
            unsafe {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, self.pipeline_kv_write);
                device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::COMPUTE, self.layout_kv_write, 0, &[self.desc_sets_kv_write[i]], &[]);
                
                let params = [pos, kv_dim as u32, max_seq_len as u32];
                let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 12);
                device.cmd_push_constants(cmd, self.layout_kv_write, vk::ShaderStageFlags::COMPUTE, 0, params_bytes);
                device.cmd_dispatch(cmd, (kv_dim as u32 + 63) / 64, 1, 1);
            }
            cmd_compute_barrier(device, cmd);

            // 2.7 Attn Softmax (read Q & KV cache, write to attn_out_buffer)
            unsafe {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, self.pipeline_attn_softmax);
                device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::COMPUTE, self.layout_attn_softmax, 0, &[self.desc_sets_attn_softmax[i]], &[]);
                
                let params = [pos, head_dim as u32, n_heads as u32, n_kv_heads as u32, max_seq_len as u32, scale.to_bits()];
                let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 24);
                device.cmd_push_constants(cmd, self.layout_attn_softmax, vk::ShaderStageFlags::COMPUTE, 0, params_bytes);
                device.cmd_dispatch(cmd, n_heads as u32, 1, 1);
            }
            cmd_compute_barrier(device, cmd);

            // 2.8 Copy attn_out_buffer to driver.shared_input_buffer
            cmd_compute_to_transfer_barrier(device, cmd);
            unsafe {
                device.cmd_copy_buffer(cmd, self.attn_out_buffer, driver.shared_input_buffer, &[vk::BufferCopy::builder().size((hidden_size * 4) as vk::DeviceSize).build()]);
            }
            cmd_transfer_to_compute_barrier(device, cmd);

            // 2.9 Output projection (read from driver.shared_input_buffer, write to o_proj.output_buffer)
            if let Some(ref g) = lg.o_proj_gpu { g.record_dispatch(cmd); }
            cmd_compute_barrier(device, cmd);

            // 2.10 Residual Add: x_residual_buffer += o_proj.output_buffer
            unsafe {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, self.pipeline_residual_add);
                device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::COMPUTE, self.layout_residual_add, 0, &[self.desc_sets_residual_add_attn[i]], &[]);
                let params = [hidden_size as u32];
                let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 4);
                device.cmd_push_constants(cmd, self.layout_residual_add, vk::ShaderStageFlags::COMPUTE, 0, params_bytes);
                device.cmd_dispatch(cmd, (hidden_size as u32 + 255) / 256, 1, 1);
            }
            cmd_compute_barrier(device, cmd);

            // 2.11 Copy x_residual_buffer to driver.shared_input_buffer
            cmd_compute_to_transfer_barrier(device, cmd);
            unsafe {
                device.cmd_copy_buffer(cmd, self.x_residual_buffer, driver.shared_input_buffer, &[vk::BufferCopy::builder().size((hidden_size * 4) as vk::DeviceSize).build()]);
            }
            cmd_transfer_to_compute_barrier(device, cmd);

            // 2.12 FFN pre-norm (in-place on driver.shared_input_buffer)
            unsafe {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, self.pipeline_rms_norm);
                device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::COMPUTE, self.layout_rms_norm, 0, &[self.desc_sets_rms_norm_ffn[i]], &[]);
                let params = [hidden_size as u32, 1e-6f32.to_bits()];
                let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
                device.cmd_push_constants(cmd, self.layout_rms_norm, vk::ShaderStageFlags::COMPUTE, 0, params_bytes);
                device.cmd_dispatch(cmd, 1, 1, 1);
            }
            cmd_compute_barrier(device, cmd);

            // 2.13 Gate/Up projections (read from driver.shared_input_buffer, write to outputs)
            if let Some(ref g) = lg.gate_proj_gpu { g.record_dispatch(cmd); }
            if let Some(ref g) = lg.up_proj_gpu { g.record_dispatch(cmd); }
            cmd_compute_barrier(device, cmd);

            // 2.14 SwiGLU (read gate/up output buffers, write to gated_buffer)
            unsafe {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, self.pipeline_swiglu);
                device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::COMPUTE, self.layout_swiglu, 0, &[self.desc_sets_swiglu[i]], &[]);
                let params = [ffn_size as u32];
                let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 4);
                device.cmd_push_constants(cmd, self.layout_swiglu, vk::ShaderStageFlags::COMPUTE, 0, params_bytes);
                device.cmd_dispatch(cmd, (ffn_size as u32 + 255) / 256, 1, 1);
            }
            cmd_compute_barrier(device, cmd);

            // 2.15 Copy gated_buffer to driver.shared_input_buffer
            cmd_compute_to_transfer_barrier(device, cmd);
            unsafe {
                device.cmd_copy_buffer(cmd, self.gated_buffer, driver.shared_input_buffer, &[vk::BufferCopy::builder().size((ffn_size * 4) as vk::DeviceSize).build()]);
            }
            cmd_transfer_to_compute_barrier(device, cmd);

            // 2.16 Down projection (read from driver.shared_input_buffer, write to down_proj.output_buffer)
            if let Some(ref g) = lg.down_proj_gpu { g.record_dispatch(cmd); }
            cmd_compute_barrier(device, cmd);

            // 2.17 Residual Add: x_residual_buffer += down_proj.output_buffer
            unsafe {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, self.pipeline_residual_add);
                device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::COMPUTE, self.layout_residual_add, 0, &[self.desc_sets_residual_add_ffn[i]], &[]);
                let params = [hidden_size as u32];
                let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 4);
                device.cmd_push_constants(cmd, self.layout_residual_add, vk::ShaderStageFlags::COMPUTE, 0, params_bytes);
                device.cmd_dispatch(cmd, (hidden_size as u32 + 255) / 256, 1, 1);
            }
            cmd_compute_barrier(device, cmd);
        }

        // 3. Final RMSNorm (in-place on x_residual_buffer)
        unsafe {
            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, self.pipeline_rms_norm);
            device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::COMPUTE, self.layout_rms_norm, 0, &[self.desc_set_final_norm], &[]);
            let params = [hidden_size as u32, 1e-6f32.to_bits()];
            let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
            device.cmd_push_constants(cmd, self.layout_rms_norm, vk::ShaderStageFlags::COMPUTE, 0, params_bytes);
            device.cmd_dispatch(cmd, 1, 1, 1);
        }
        cmd_compute_to_host_barrier(device, cmd);

        // 4. End Command Buffer
        unsafe {
            device.end_command_buffer(cmd)?;
        }

        // 5. Submit Command Buffer
        unsafe {
            device.reset_fences(&[self.fence])?;
            device.queue_submit(self.queue, &[vk::SubmitInfo::builder().command_buffers(&[cmd]).build()], self.fence)?;
            device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }

        Ok(())
    }
}

impl Drop for VulkanModelPipeline {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_descriptor_pool(self.desc_pool, None);
            
            let destroy_buffer_fn = |device: &Device, buffer: vk::Buffer, memory: vk::DeviceMemory, mapped: bool| {
                if mapped {
                    device.unmap_memory(memory);
                }
                device.free_memory(memory, None);
                device.destroy_buffer(buffer, None);
            };

            destroy_buffer_fn(&self.device, self.x_residual_buffer, self.x_residual_memory, true);
            destroy_buffer_fn(&self.device, self.attn_out_buffer, self.attn_out_memory, false);
            destroy_buffer_fn(&self.device, self.gated_buffer, self.gated_memory, false);

            for (buf, mem) in &self.layer_attn_norms { destroy_buffer_fn(&self.device, *buf, *mem, false); }
            for (buf, mem) in &self.layer_ffn_norms { destroy_buffer_fn(&self.device, *buf, *mem, false); }
            for bias in &self.layer_q_biases { if let Some((buf, mem)) = bias { destroy_buffer_fn(&self.device, *buf, *mem, false); } }
            for bias in &self.layer_k_biases { if let Some((buf, mem)) = bias { destroy_buffer_fn(&self.device, *buf, *mem, false); } }
            for bias in &self.layer_v_biases { if let Some((buf, mem)) = bias { destroy_buffer_fn(&self.device, *buf, *mem, false); } }
            destroy_buffer_fn(&self.device, self.final_norm_buf, self.final_norm_mem, false);

            for (buf, mem) in &self.layer_kv_caches { destroy_buffer_fn(&self.device, *buf, *mem, false); }

            self.device.destroy_pipeline(self.pipeline_rms_norm, None);
            self.device.destroy_pipeline(self.pipeline_rope, None);
            self.device.destroy_pipeline(self.pipeline_kv_write, None);
            self.device.destroy_pipeline(self.pipeline_attn_softmax, None);
            self.device.destroy_pipeline(self.pipeline_swiglu, None);
            self.device.destroy_pipeline(self.pipeline_residual_add, None);
            self.device.destroy_pipeline(self.pipeline_bias_add, None);

            self.device.destroy_pipeline_layout(self.layout_rms_norm, None);
            self.device.destroy_pipeline_layout(self.layout_rope, None);
            self.device.destroy_pipeline_layout(self.layout_kv_write, None);
            self.device.destroy_pipeline_layout(self.layout_attn_softmax, None);
            self.device.destroy_pipeline_layout(self.layout_swiglu, None);
            self.device.destroy_pipeline_layout(self.layout_residual_add, None);
            self.device.destroy_pipeline_layout(self.layout_bias_add, None);

            self.device.destroy_descriptor_set_layout(self.desc_layout_2, None);
            self.device.destroy_descriptor_set_layout(self.desc_layout_3, None);

            self.device.destroy_shader_module(self.shader_rms_norm, None);
            self.device.destroy_shader_module(self.shader_rope, None);
            self.device.destroy_shader_module(self.shader_kv_write, None);
            self.device.destroy_shader_module(self.shader_attn_softmax, None);
            self.device.destroy_shader_module(self.shader_swiglu, None);
            self.device.destroy_shader_module(self.shader_residual_add, None);
            self.device.destroy_shader_module(self.shader_bias_add, None);
        }
    }
}

// ─── Vulkan Helpers ─────────────────────────────────────────────────────────

fn create_shader_module(device: &Device, spv_code: &[u32]) -> Result<vk::ShaderModule, Box<dyn std::error::Error>> {
    let shader_info = vk::ShaderModuleCreateInfo::builder().code(spv_code);
    let shader_module = unsafe { device.create_shader_module(&shader_info, None)? };
    Ok(shader_module)
}

fn create_desc_layout(device: &Device, bindings: &[vk::DescriptorSetLayoutBinding]) -> Result<vk::DescriptorSetLayout, Box<dyn std::error::Error>> {
    let layout_info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(bindings);
    let layout = unsafe { device.create_descriptor_set_layout(&layout_info, None)? };
    Ok(layout)
}

fn create_pipeline_layout(
    device: &Device,
    desc_set_layout: vk::DescriptorSetLayout,
    push_constant_size: u32,
) -> Result<vk::PipelineLayout, Box<dyn std::error::Error>> {
    let push_constant_ranges = if push_constant_size > 0 {
        vec![vk::PushConstantRange::builder()
            .stage_flags(vk::ShaderStageFlags::COMPUTE)
            .offset(0)
            .size(push_constant_size)
            .build()]
    } else {
        vec![]
    };
    let layouts = [desc_set_layout];
    let layout_info = vk::PipelineLayoutCreateInfo::builder()
        .set_layouts(&layouts)
        .push_constant_ranges(&push_constant_ranges);
    let layout = unsafe { device.create_pipeline_layout(&layout_info, None)? };
    Ok(layout)
}

fn create_compute_pipeline(
    device: &Device,
    shader_module: vk::ShaderModule,
    layout: vk::PipelineLayout,
) -> Result<vk::Pipeline, Box<dyn std::error::Error>> {
    let main_entry = CString::new("main")?;
    let stage_info = vk::PipelineShaderStageCreateInfo::builder()
        .stage(vk::ShaderStageFlags::COMPUTE)
        .module(shader_module)
        .name(&main_entry);
    let pipeline_info = vk::ComputePipelineCreateInfo::builder()
        .stage(stage_info.build())
        .layout(layout);
    let pipelines = unsafe {
        device.create_compute_pipelines(vk::PipelineCache::null(), &[pipeline_info.build()], None)
            .map_err(|(_, e)| e)?
    };
    Ok(pipelines[0])
}

fn create_uninitialized_device_local_buffer(
    device: &Device,
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
    size: vk::DeviceSize,
    usage: vk::BufferUsageFlags,
) -> Result<(vk::Buffer, vk::DeviceMemory), Box<dyn std::error::Error>> {
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
            && flags.contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
        {
            mem_type_index = Some(i);
            break;
        }
    }
    let memory_type_index = mem_type_index.ok_or("No device-local memory found on GPU.")?;
    let alloc_info = vk::MemoryAllocateInfo::builder()
        .allocation_size(mem_reqs.size)
        .memory_type_index(memory_type_index);
    let memory = unsafe { device.allocate_memory(&alloc_info, None)? };
    unsafe { device.bind_buffer_memory(buffer, memory, 0)? };
    Ok((buffer, memory))
}

fn cmd_compute_barrier(device: &Device, cmd: vk::CommandBuffer) {
    let memory_barrier = vk::MemoryBarrier::builder()
        .src_access_mask(vk::AccessFlags::SHADER_WRITE)
        .dst_access_mask(vk::AccessFlags::SHADER_READ | vk::AccessFlags::SHADER_WRITE);
        
    unsafe {
        device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::DependencyFlags::empty(),
            &[memory_barrier.build()],
            &[],
            &[],
        );
    }
}

fn cmd_transfer_to_compute_barrier(device: &Device, cmd: vk::CommandBuffer) {
    let memory_barrier = vk::MemoryBarrier::builder()
        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
        .dst_access_mask(vk::AccessFlags::SHADER_READ);
        
    unsafe {
        device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::DependencyFlags::empty(),
            &[memory_barrier.build()],
            &[],
            &[],
        );
    }
}

fn cmd_compute_to_transfer_barrier(device: &Device, cmd: vk::CommandBuffer) {
    let memory_barrier = vk::MemoryBarrier::builder()
        .src_access_mask(vk::AccessFlags::SHADER_WRITE)
        .dst_access_mask(vk::AccessFlags::TRANSFER_READ);
        
    unsafe {
        device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[memory_barrier.build()],
            &[],
            &[],
        );
    }
}

fn cmd_compute_to_host_barrier(device: &Device, cmd: vk::CommandBuffer) {
    let memory_barrier = vk::MemoryBarrier::builder()
        .src_access_mask(vk::AccessFlags::SHADER_WRITE)
        .dst_access_mask(vk::AccessFlags::HOST_READ);
        
    unsafe {
        device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::PipelineStageFlags::HOST,
            vk::DependencyFlags::empty(),
            &[memory_barrier.build()],
            &[],
            &[],
        );
    }
}


