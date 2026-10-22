#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// 1. Declare UI parameters in the JSON manifest
const MANIFEST: &str = r#"{
"id": "org.photocraft.filter.neural_hallucination",
"name": "Domain Warped Neural Hallucination",
"version": "1.0.0",
"kind": "filter",
"author": "CHJ85",
"description": "Recursive domain warping that transforms images into surreal, AI-resembling neural dreamscapes.",
"params": {
"warp_intensity": {
"type": "number",
"default": 2.5,
"min": 0.5,
"max": 6.0,
"label": "Latent Warp"
},
"octaves": {
"type": "int",
"default": 3,
"min": 1,
"max": 5,
"label": "Complexity Octaves"
},
"brightness": {
"type": "number",
"default": 1.1,
"min": 0.5,
"max": 2.0,
"label": "Brightness"
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

/// Fast no_std math helpers
fn abs_f32(val: f32) -> f32 {
    if val < 0.0 { -val } else { val }
}

fn fract_f32(val: f32) -> f32 {
    val - (val as i32) as f32
}

fn sin_f32(mut x: f32) -> f32 {
    let pi = 3.141592653589793f32;
    let two_pi = 6.283185307179586f32;
    x = x % two_pi;
    if x > pi { x -= two_pi; }
    else if x < -pi { x += two_pi; }

    let x2 = x * x;
    let x3 = x2 * x;
    let x5 = x3 * x2;
    let x7 = x5 * x2;
    x - x3 / 6.0 + x5 / 120.0 - x7 / 5040.0
}

fn cos_f32(x: f32) -> f32 {
    let pi_half = 1.5707963267948966f32;
    sin_f32(x + pi_half)
}

/// Fast deterministic pseudo-random hash generator for no_std
fn hash_noise(mut n: f32) -> f32 {
    n = fract_f32(n * 0.1031);
    n *= n + 33.33;
    n = (n * n + n) * 0.0871;
    fract_f32(n)
}

fn procedural_noise(x: f32, y: f32) -> f32 {
    let x_int = x as i32;
    let y_int = y as i32;
    let x_frac = x - x_int as f32;
    let y_frac = y - y_int as f32;

    let n00 = hash_noise((x_int as f32) * 12.9898 + (y_int as f32) * 78.233);
    let n10 = hash_noise((x_int as f32 + 1.0) * 12.9898 + (y_int as f32) * 78.233);
    let n01 = hash_noise((x_int as f32) * 12.9898 + (y_int as f32 + 1.0) * 78.233);
    let n11 = hash_noise((x_int as f32 + 1.0) * 12.9898 + (y_int as f32 + 1.0) * 78.233);

    // Smoothstep interpolation
    let sx = x_frac * x_frac * (3.0 - 2.0 * x_frac);
    let sy = y_frac * y_frac * (3.0 - 2.0 * y_frac);

    let nx0 = n00 * (1.0 - sx) + n10 * sx;
    let nx1 = n01 * (1.0 - sx) + n11 * sx;
    nx0 * (1.0 - sy) + nx1 * sy
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

    let warp_intensity = parse_param_f32(param_bytes, b"warp_intensity", 2.5);
    let octaves_param = parse_param_f32(param_bytes, b"octaves", 3.0) as usize;
    let brightness = parse_param_f32(param_bytes, b"brightness", 1.1);
    let octaves = octaves_param.clamp(1, 5);

    // Allocate temp scratch buffer for source reference
    let bytes_needed = (total_samples * 4) as u32;
    let src_offset = pc_alloc(bytes_needed);
    if src_offset == 0 {
        return 1;
    }
    let src_ptr = src_offset as *mut f32;
    let src_buf = unsafe { core::slice::from_raw_parts_mut(src_ptr, total_samples) };
    src_buf.copy_from_slice(px);

    // Apply Domain Warped Neural Hallucination
    for y in 0..h {
        let fy = y as f32 * 0.01;
        for x in 0..w {
            let fx = x as f32 * 0.01;
            let dst_idx = (y * w + x) * ch;

            if has_alpha && src_buf[dst_idx + ch - 1] <= 0.0 {
                continue;
            }

            // Recursive Domain Warping Calculation
            let mut qx = fx;
            let mut qy = fy;
            let mut amp = 1.0;
            let mut freq = 1.0;

            for _ in 0..octaves {
                qx += procedural_noise(fx * freq + qy, fy * freq - qx) * amp * warp_intensity;
                qy += procedural_noise(fx * freq - qx, fy * freq + qy) * amp * warp_intensity;
                amp *= 0.5;
                freq *= 2.0;
            }

            // Map warped coordinates back into source buffer with wrap/clamp
            let sample_x = ((qx * 50.0) as usize) % w;
            let sample_y = ((qy * 50.0) as usize) % h;
            let src_idx = (sample_y * w + sample_x) * ch;

            // Blend original color with warped neural feedback using custom math helpers
            let base_r = src_buf[dst_idx];
            let base_g = src_buf[dst_idx + 1];
            let base_b = src_buf[dst_idx + 2];

            let warp_r = src_buf[src_idx];
            let warp_g = src_buf[src_idx + 1];
            let warp_b = src_buf[src_idx + 2];

            let final_r = (base_r * 0.4 + warp_r * 0.6 * (abs_f32(sin_f32(qx)) + 0.5)) * brightness;
            let final_g = (base_g * 0.4 + warp_g * 0.6 * (abs_f32(cos_f32(qy)) + 0.5)) * brightness;
            let final_b = (base_b * 0.4 + warp_b * 0.6 * (abs_f32(sin_f32(qx + qy)) + 0.5)) * brightness;

            px[dst_idx] = final_r.clamp(0.0, 1.0);
            px[dst_idx + 1] = final_g.clamp(0.0, 1.0);
            px[dst_idx + 2] = final_b.clamp(0.0, 1.0);

            if has_alpha {
                px[dst_idx + ch - 1] = src_buf[dst_idx + ch - 1];
            }
        }
    }

    0
}
