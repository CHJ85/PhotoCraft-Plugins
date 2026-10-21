#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// 1. Declare UI parameters in the JSON manifest
const MANIFEST: &str = r#"{
"id": "org.photocraft.filter.reaction_diffusion",
"name": "Reaction-Diffusion Morphogenesis",
"version": "1.0.0",
"kind": "filter",
"author": "CHJ85",
"description": "Simulates chemical reaction-diffusion differential equations to grow biological patterns from image data.",
"params": {
"feed": {
"type": "number",
"default": 0.055,
"min": 0.01,
"max": 0.09,
"label": "Feed Rate"
},
"kill": {
"type": "number",
"default": 0.062,
"min": 0.04,
"max": 0.07,
"label": "Kill Rate"
},
"iterations": {
"type": "int",
"default": 3,
"min": 1,
"max": 8,
"label": "Growth Steps"
}
}
}"#;

const HAS_ALPHA: u32 = 1 << 8;

#[unsafe(no_mangle)]
pub extern "C" fn pc_abi_version() -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn pc_manifest() -> u64 {
    ((MANIFEST.len() as u64) << 32) | MANIFEST.as_ptr() as u64
}

/// Dynamic allocator using WASM intrinsic memory_grow
#[unsafe(no_mangle)]
pub extern "C" fn pc_alloc(size: u32) -> u32 {
    let pages = (size as usize).div_ceil(65536);
    let old = core::arch::wasm32::memory_grow(0, pages);
    if old == usize::MAX {
        0
    } else {
        (old * 65536) as u32
    }
}

/// Robust JSON parameter parser matching keys inside incoming JSON payloads
fn parse_param_f32(json: &[u8], key: &[u8], default: f32) -> f32 {
    if json.is_empty() || key.is_empty() {
        return default;
    }

    let mut i = 0;
    while i + key.len() + 2 < json.len() {
        if &json[i..i + key.len()] == key {
            if i > 0 && json[i - 1] == b'"' {
                let mut idx = i + key.len();
                while idx < json.len() && (json[idx] == b'"' || json[idx] == b':' || json[idx] == b' ' || json[idx] == b'\t' || json[idx] == b'\n' || json[idx] == b'\r') {
                    idx += 1;
                }

                let start = idx;
                while idx < json.len() && json[idx] != b',' && json[idx] != b'}' && json[idx] != b'"' && json[idx] != b' ' {
                    idx += 1;
                }

                if idx > start {
                    if let Some(val) = parse_f32_slice(&json[start..idx]) {
                        return val;
                    }
                }
            }
        }
        i += 1;
    }
    default
}

fn parse_f32_slice(bytes: &[u8]) -> Option<f32> {
    let mut val = 0.0f32;
    let mut decimal = false;
    let mut div = 10.0f32;
    let mut negative = false;
    let mut has_digits = false;

    for (idx, &b) in bytes.iter().enumerate() {
        if idx == 0 && b == b'-' {
            negative = true;
            continue;
        }
        if b == b'.' {
            decimal = true;
            continue;
        }
        if b < b'0' || b > b'9' {
            break;
        }
        has_digits = true;
        let digit = (b - b'0') as f32;
        if !decimal {
            val = val * 10.0 + digit;
        } else {
            val += digit / div;
            div *= 10.0;
        }
    }

    if !has_digits {
        return None;
    }

    Some(if negative { -val } else { val })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pc_filter(
    buf: *mut f32,
    len: u32,
    width: u32,
    height: u32,
    channels: u32,
    format: u32,
        params: *const u8,
        params_len: u32,
) -> i32 {
    let w = width as usize;
    let h = height as usize;
    let ch = channels as usize;

    if ch < 3 || w == 0 || h == 0 || buf.is_null() {
        return 1;
    }

    let total_samples = len as usize / 4;
    let px = unsafe { core::slice::from_raw_parts_mut(buf, total_samples) };
    let has_alpha = format & HAS_ALPHA != 0;

    // Extract dynamic parameter values sent by host sliders
    let param_bytes = if !params.is_null() && params_len > 0 {
        unsafe { core::slice::from_raw_parts(params, params_len as usize) }
    } else {
        &[]
    };

    let feed = parse_param_f32(param_bytes, b"feed", 0.055);
    let kill = parse_param_f32(param_bytes, b"kill", 0.062);
    let iterations_param = parse_param_f32(param_bytes, b"iterations", 3.0) as usize;
    let iterations = iterations_param.clamp(1, 8);

    let num_pixels = w * h;
    let bytes_needed = (num_pixels * 4) as u32;

    // Allocate chemical concentration buffers (A and B chemicals for Gray-Scott model)
    let a_offset = pc_alloc(bytes_needed);
    let b_offset = pc_alloc(bytes_needed);
    let next_a_offset = pc_alloc(bytes_needed);
    let next_b_offset = pc_alloc(bytes_needed);

    if a_offset == 0 || b_offset == 0 || next_a_offset == 0 || next_b_offset == 0 {
        return 1;
    }

    let a = unsafe { core::slice::from_raw_parts_mut(a_offset as *mut f32, num_pixels) };
    let b = unsafe { core::slice::from_raw_parts_mut(b_offset as *mut f32, num_pixels) };
    let next_a = unsafe { core::slice::from_raw_parts_mut(next_a_offset as *mut f32, num_pixels) };
    let next_b = unsafe { core::slice::from_raw_parts_mut(next_b_offset as *mut f32, num_pixels) };

    // Initialize chemical concentrations: A = 1.0 everywhere, B seeded by image luminance
    for y in 0..h {
        for x in 0..w {
            let p_idx = y * w + x;
            let px_idx = p_idx * ch;
            let luma = 0.2126 * px[px_idx] + 0.7152 * px[px_idx + 1] + 0.0722 * px[px_idx + 2];

            a[p_idx] = 1.0;
            b[p_idx] = if luma > 0.6 { 1.0 } else { 0.0 };
        }
    }

    // Diffusion coefficients for Gray-Scott reaction-diffusion
    let da = 1.0f32;
    let db = 0.5f32;
    let dt = 1.0f32;

    // Run numerical differential equation solver iterations
    for _ in 0..iterations {
        for y in 0..h {
            let y_up = y.saturating_sub(1);
            let y_down = (y + 1).min(h - 1);

            for x in 0..w {
                let x_left = x.saturating_sub(1);
                let x_right = (x + 1).min(w - 1);
                let p_idx = y * w + x;

                let center_a = a[p_idx];
                let center_b = b[p_idx];

                // Laplacian Bergen/grid convolution stencil
                let lap_a = a[y_up * w + x] + a[y_down * w + x] + a[y * w + x_left] + a[y * w + x_right] - 4.0 * center_a;
                let lap_b = b[y_up * w + x] + b[y_down * w + x] + b[y * w + x_left] + b[y * w + x_right] - 4.0 * center_b;

                let abb = center_a * center_b * center_b;

                next_a[p_idx] = (center_a + (da * lap_a - abb + feed * (1.0 - center_a)) * dt).clamp(0.0, 1.0);
                next_b[p_idx] = (center_b + (db * lap_b + abb - (kill + feed) * center_b) * dt).clamp(0.0, 1.0);
            }
        }

        // Swap buffers for next step
        a.copy_from_slice(next_a);
        b.copy_from_slice(next_b);
    }

    // Render resulting chemical concentration 'B' back to image pixels with an organic color map
    for y in 0..h {
        for x in 0..w {
            let p_idx = y * w + x;
            let px_idx = p_idx * ch;

            if has_alpha && px[px_idx + ch - 1] <= 0.0 {
                continue;
            }

            let chem = b[p_idx];

            // Organic duotone mapping based on chemical concentration
            px[px_idx] = (chem * 1.2).clamp(0.0, 1.0);
            px[px_idx + 1] = (chem * 0.6 + (1.0 - chem) * 0.2).clamp(0.0, 1.0);
            px[px_idx + 2] = ((1.0 - chem) * 0.8).clamp(0.0, 1.0);

            if has_alpha {
                px[px_idx + ch - 1] = px[px_idx + ch - 1];
            }
        }
    }

    0
}
