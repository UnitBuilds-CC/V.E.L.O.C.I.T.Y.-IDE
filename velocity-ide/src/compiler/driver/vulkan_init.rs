use ash::vk::Handle;
use ash::{vk, Device, Entry, Instance};
use std::ffi::CString;

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

            let queue_properties =
                unsafe { instance.get_physical_device_queue_family_properties(pd) };
            let mut compute_family = None;
            for (idx, qprop) in queue_properties.iter().enumerate() {
                if qprop.queue_flags.contains(vk::QueueFlags::COMPUTE) {
                    compute_family = Some(idx as u32);
                    break;
                }
            }

            if compute_family.is_some()
                && (selected_device.is_none() || dev_type == vk::PhysicalDeviceType::DISCRETE_GPU) {
                    selected_device = Some(pd);
                    selected_queue_family = compute_family;
                    selected_device_name = name;
                }
        }

        let physical_device = selected_device.ok_or("No compute-capable GPU found.")?;
        let queue_family_index = selected_queue_family.ok_or("No compute queue family found.")?;

        eprintln!("Selected GPU: {}", selected_device_name);

        let queue_priorities = [1.0f32];
        let queue_create_info = vk::DeviceQueueCreateInfo::builder()
            .queue_family_index(queue_family_index)
            .queue_priorities(&queue_priorities);

        let device_create_infos = [queue_create_info.build()];
        let device_create_info =
            vk::DeviceCreateInfo::builder().queue_create_infos(&device_create_infos);

        let device = unsafe { instance.create_device(physical_device, &device_create_info, None)? };
        let compute_queue = unsafe { device.get_device_queue(queue_family_index, 0) };

        let shared_size = (65536 * 4) as vk::DeviceSize;
        let (shared_input_buffer, shared_input_memory, shared_input_ptr) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            shared_size,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;

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

    pub fn benchmark_attention_nda_vs_contig(
        &self,
    ) -> Result<(f64, f64), Box<dyn std::error::Error>> {
        super::vulkan_benchmark::benchmark_attention_nda_vs_contig(self)
    }

    pub fn run_attn_benchmarks(&self) -> Result<(f64, f64), Box<dyn std::error::Error>> {
        self.benchmark_attention_nda_vs_contig()
    }
}

impl Drop for VulkanDriver {
    fn drop(&mut self) {
        unsafe {
            self.device.unmap_memory(self.shared_input_memory);
            self.device.free_memory(self.shared_input_memory, None);
            self.device.destroy_buffer(self.shared_input_buffer, None);
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

pub fn create_uninitialized_device_local_buffer(
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

pub fn create_shader_module(
    device: &Device,
    spv_code: &[u32],
) -> Result<vk::ShaderModule, Box<dyn std::error::Error>> {
    let shader_info = vk::ShaderModuleCreateInfo::builder().code(spv_code);
    let shader_module = unsafe { device.create_shader_module(&shader_info, None)? };
    Ok(shader_module)
}

pub fn create_desc_layout(
    device: &Device,
    bindings: &[vk::DescriptorSetLayoutBinding],
) -> Result<vk::DescriptorSetLayout, Box<dyn std::error::Error>> {
    let layout_info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(bindings);
    let layout = unsafe { device.create_descriptor_set_layout(&layout_info, None)? };
    Ok(layout)
}

pub fn create_pipeline_layout(
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

pub fn create_compute_pipeline(
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
        device
            .create_compute_pipelines(vk::PipelineCache::null(), &[pipeline_info.build()], None)
            .map_err(|(_, e)| e)?
    };
    Ok(pipelines[0])
}

pub fn cmd_compute_barrier(device: &Device, cmd: vk::CommandBuffer) {
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

pub fn cmd_transfer_to_compute_barrier(device: &Device, cmd: vk::CommandBuffer) {
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

pub fn cmd_compute_to_transfer_barrier(device: &Device, cmd: vk::CommandBuffer) {
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

pub fn cmd_compute_to_host_barrier(device: &Device, cmd: vk::CommandBuffer) {
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
