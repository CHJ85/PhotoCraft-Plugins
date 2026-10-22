#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// 1. Declare UI parameters in the JSON manifest
const MANIFEST: &str = r#"{
"id": "org.photocraft.filter.anisotropic_smoothing",
"name": "Tensor Anisotropic Smoothing",
"version": "1.0.0",
"kind": "filter",
"author": "CHJ85",
"description": "PDE-based anisotropic diffusion that smooths noise along edges while preserving sharp boundaries.",
"params": {
"smoothness": {
"type": "number",
"default": 1.5,
"min": 0.5,
"max": 4.0,
"label": "Diffusion Power"
},
"iterations": {
"type": "int",
"default": 3,
"min": 1,
"max": 8,
"label": "Iterations"
},
"brightness": {
"type": "number",
"default": 1.0,
"min": 0.5,
"max": 1.5,
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

    let smoothness = parse_param_f32(param_bytes, b"smoothness", 1.5);
    let iterations_param = parse_param_f32(param_bytes, b"iterations", 3.0) as usize;
    let brightness = parse_param_f32(param_bytes, b"brightness", 1.0);
    let iterations = iterations_param.clamp(1, 8);

    // Allocate temp scratch buffers for iterative PDE diffusion
    let bytes_needed = (total_samples * 4) as u32;
    let work_offset = pc_alloc(bytes_needed);
    let next_offset = pc_alloc(bytes_needed);

    if work_offset == 0 || next_offset == 0 {
        return 1;
    }

    let work_buf = unsafe { core::slice::from_raw_parts_mut(work_offset as *mut f32, total_samples) };
    let next_buf = unsafe { core::slice::from_raw_parts_mut(next_offset as *mut f32, total_samples) };
    work_buf.copy_from_slice(px);
    next_buf.copy_from_slice(px);

    // Iterative anisotropic diffusion solver
    for _ in 0..iterations {
        for y in 0..h {
            let y_prev = y.saturating_sub(1);
            let y_next = (y + 1).min(h - 1);

            for x in 0..w {
                let x_prev = x.saturating_sub(1);
                let x_next = (x + 1).min(w - 1);
                let idx = (y * w + x) * ch;

                if has_alpha && work_buf[idx + ch - 1] <= 0.0 {
                    continue;
                }

                for c in 0..3 {
                    let center = work_buf[idx + c];
                    let up = work_buf[(y_prev * w + x) * ch + c];
                    let down = work_buf[(y_next * w + x) * ch + c];
                    let left = work_buf[(y * w + x_prev) * ch + c];
                    let right = work_buf[(y * w + x_next) * ch + c];

                    // Gradient fluxes
                    let grad_n = up - center;
                    let grad_s = down - center;
                    let grad_w = left - center;
                    let grad_e = right - center;

                    // Conduction coefficients (Perona-Malik edge-stopping function)
                    let k = smoothness;
                    let c_n = 1.0 / (1.0 + (grad_n * grad_n) / (k * k));
                    let c_s = 1.0 / (1.0 + (grad_s * grad_s) / (k * k));
                    let c_w = 1.0 / (1.0 + (grad_w * grad_w) / (k * k));
                    let c_e = 1.0 / (1.0 + (grad_e * grad_e) / (k * k));

                    let diffused = center + 0.25 * (c_n * grad_n + c_s * grad_s + c_w * grad_w + c_e * grad_e);
                    next_buf[idx + c] = diffused.clamp(0.0, 1.0);
                }

                if has_alpha {
                    next_buf[idx + ch - 1] = work_buf[idx + ch - 1];
                }
            }
        }
        work_buf.copy_from_slice(next_buf);
    }

    // Copy result back to main buffer with brightness adjustment
    for i in 0..total_samples {
        px[i] = (work_buf[i] * brightness).clamp(0.0, 1.0);
    }

    0
}
