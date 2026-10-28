// One sparse air cell per invocation: centre, four masked neighbours,
// number of open faces, flower emission, padding. Solid faces reflect air.
struct Cell { a: vec4<f32>, b: vec4<f32> }
@group(0) @binding(0) var<storage, read> cells: array<Cell>;
@group(0) @binding(1) var<storage, read_write> result: array<f32>;

@compute @workgroup_size(64)
fn diffuse(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let i = id.x + id.y * groups.x * 64u;
    if i >= arrayLength(&cells) { return; }
    let c = cells[i];
    let neighbours = ((c.a.y + c.a.z) + c.a.w) + c.b.x;
    let spread = c.a.x + 0.16 * (neighbours - c.b.y * c.a.x);
    let value = clamp(spread * 0.95 + c.b.z, -10.0, 10.0);
    result[i] = select(value, 0.0, abs(value) < 0.01);
}
