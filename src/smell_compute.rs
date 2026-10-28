//! Compute-only wgpu adapter. No Bevy app or render-frame dependency: a tick
//! reads its own completed field, including in the headless lab and QA ticks.
use std::{
    future::Future,
    sync::{Arc, Mutex, OnceLock},
    task::{Context, Poll, Wake, Waker},
};

fn block_on<T>(future: impl Future<Output = T>) -> T {
    struct Thread(std::thread::Thread);
    impl Wake for Thread {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(Thread(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}

struct Compute {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    buffers: Option<(usize, wgpu::Buffer, wgpu::Buffer, wgpu::Buffer)>,
}

impl Compute {
    fn new() -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("smell compute needs a GPU adapter");
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("create smell compute device");
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("smell diffusion"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../assets/shaders/smell.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("smell diffusion"),
            layout: None,
            module: &module,
            entry_point: Some("diffuse"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self {
            device,
            queue,
            pipeline,
            buffers: None,
        }
    }

    fn run(&mut self, cells: &[[f32; 8]], result: &mut [f32]) {
        if cells.is_empty() {
            return;
        }
        let n = cells.len();
        // Retain the device, pipeline and buffers between field steps.
        if self.buffers.as_ref().is_none_or(|b| b.0 < n) {
            let capacity = n.next_power_of_two();
            let input = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("smell cells"),
                size: (capacity * 32) as u64,
                mapped_at_creation: false,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            });
            let make = |label, usage| {
                self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(label),
                    size: (capacity * 4) as u64,
                    usage,
                    mapped_at_creation: false,
                })
            };
            self.buffers = Some((
                capacity,
                input,
                make(
                    "smell output",
                    wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                ),
                make(
                    "smell readback",
                    wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                ),
            ));
        }
        let (_, input, output, readback) = self.buffers.as_ref().unwrap();
        self.queue
            .write_buffer(input, 0, bytemuck::cast_slice(cells));
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("smell"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: input,
                        offset: 0,
                        size: wgpu::BufferSize::new((n * 32) as u64),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: output,
                        offset: 0,
                        size: wgpu::BufferSize::new((n * 4) as u64),
                    }),
                },
            ],
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            // Use two dimensions so large fields never exceed dispatch limits.
            let groups = (n as u32).div_ceil(64);
            let width = groups.min(self.device.limits().max_compute_workgroups_per_dimension);
            pass.dispatch_workgroups(width, groups.div_ceil(width), 1);
        }
        encoder.copy_buffer_to_buffer(output, 0, readback, 0, (n * 4) as u64);
        self.queue.submit([encoder.finish()]);
        let (send, recv) = std::sync::mpsc::sync_channel(1);
        readback
            .slice(..(n * 4) as u64)
            .map_async(wgpu::MapMode::Read, move |r| {
                let _ = send.send(r);
            });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(10)),
            })
            .expect("smell compute timed out");
        recv.recv()
            .expect("smell readback callback")
            .expect("smell readback");
        {
            let bytes = readback.slice(..(n * 4) as u64).get_mapped_range();
            result.copy_from_slice(bytemuck::cast_slice(&bytes));
        }
        readback.unmap();
    }
}

// Process lifetime, like the graphics driver itself. Dropping a wgpu device
// from a TLS destructor can call driver/tracing TLS that has already exited.
// Only the sequential world-field step takes this lock; agents never do.
static COMPUTE: OnceLock<Mutex<Compute>> = OnceLock::new();

pub fn evolve(cells: &[[f32; 8]], result: &mut [f32]) {
    assert_eq!(cells.len(), result.len());
    // Unit simulations stay GPU-independent; the explicit GPU test below
    // checks this reference against the real shader. Game and lab use GPU.
    #[cfg(test)]
    {
        reference(cells, result);
    }
    #[cfg(not(test))]
    {
        gpu(cells, result);
    }
}

fn gpu(cells: &[[f32; 8]], result: &mut [f32]) {
    if cells.is_empty() {
        return;
    }
    COMPUTE
        .get_or_init(|| Mutex::new(Compute::new()))
        .lock()
        .expect("smell compute lock")
        .run(cells, result);
}

#[cfg(test)]
fn reference(cells: &[[f32; 8]], result: &mut [f32]) {
    for (c, out) in cells.iter().zip(result) {
        let sum = ((c[1] + c[2]) + c[3]) + c[4];
        let value = ((c[0] + 0.16 * (sum - c[5] * c[0])) * 0.95 + c[6]).clamp(-10.0, 10.0);
        *out = if value.abs() < 0.01 { 0.0 } else { value };
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn gpu_smell_matches_reference_for_signed_diffusion_decay_and_cutoff() {
        let cells: Vec<_> = (0..257)
            .map(|i| {
                let value = (i as f32 - 128.0) / 13.0;
                [value, -value, value * 0.3, 0.001, 0.0, 4.0, 0.25, 0.0]
            })
            .chain([
                [0.001, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                [10.0, 10.0, 10.0, 10.0, 10.0, 4.0, 2.0, 0.0],
            ])
            .collect();
        let mut expected = vec![0.0; cells.len()];
        let mut actual = expected.clone();
        super::reference(&cells, &mut expected);
        super::gpu(&cells, &mut actual);
        for (a, b) in actual.iter().zip(expected) {
            assert!((a - b).abs() < 0.00001, "{a} != {b}");
        }
    }
}
