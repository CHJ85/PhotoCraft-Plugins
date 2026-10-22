#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// 1. Declare UI parameters in the JSON manifest
const MANIFEST: &str = r#"{
"id": "org.photocraft.filter.richardson_lucy",
"name": "Richardson-Lucy Deconvolution",
"version": "1.0.0",
"kind": "filter",
"author": "CHJ85",
"description": "Bayesian optical deconvolution to mathematically reverse blur and restore high-frequency detail.",
"params": {
"iterations": {
"type": "int",
"default": 4,
"min": 1,
"max": 10,
"label": "Deblur Passes"
},
"psf_radius": {
"type": "int",
"default": 2,
"min": 1,
"max": 5,
"label": "Blur Kernel (PSF)"
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

    let iterations_param = parse_param_f32(param_bytes, b"iterations", 4.0) as usize;
    let r_param = parse_param_f32(param_bytes, b"psf_radius", 2.0) as usize;
    let brightness = parse_param_f32(param_bytes, b"brightness", 1.0);

    let iterations = iterations_param.clamp(1, 10);
    let psf_r = r_param.clamp(1, 5);

    // Allocate temp scratch buffers for Bayesian estimation steps
    let bytes_needed = (total_samples * 4) as u32;
    let est_offset = pc_alloc(bytes_needed);
    let blur_est_offset = pc_alloc(bytes_needed);

    if est_offset == 0 || blur_est_offset == 0 {
        return 1;
    }

    let estimate = unsafe { core::slice::from_raw_parts_mut(est_offset as *mut f32, total_samples) };
    let blurred_estimate = unsafe { core::slice::from_raw_parts_mut(blur_est_offset as *mut f32, total_samples) };

    // Initialize estimate with original pixel values
    estimate.copy_from_slice(px);

    // Iterative Richardson-Lucy Deconvolution loop
    for _ in 0..iterations {
        // Step 1: Convolve current estimate with PSF kernel
        box_blur_kernel(estimate, blurred_estimate, w, h, ch, psf_r, has_alpha);

        // Step 2: Update estimate based on ratio of observed image to blurred estimate
        for y in 0..h {
            for x in 0..w {
                let idx = (y * w + x) * ch;

                if has_alpha && px[idx + ch - 1] <= 0.0 {
                    continue;
                }

                for c in 0..3 {
                    let observed = px[idx + c];
                    let modeled = blurred_estimate[idx + c] + 0.0001; // Avoid division by zero
                    let ratio = observed / modeled;

                    // Bayesian multiplicative correction factor
                    estimate[idx + c] = (estimate[idx + c] * ratio).clamp(0.0, 1.0);
                }

                if has_alpha {
                    estimate[idx + ch - 1] = px[idx + ch - 1];
                }
            }
        }
    }

    // Copy reconstructed image back to main buffer with brightness adjustment
    for i in 0..total_samples {
        px[i] = (estimate[i] * brightness).clamp(0.0, 1.0);
    }

    0
}

fn box_blur_kernel(src: &[f32], dst: &mut [f32], w: usize, h: usize, ch: usize, r: usize, has_alpha: bool) {
    for y in 0..h {
        let y_min = y.saturating_sub(r);
        let y_max = (y + r).min(h - 1);

        for x in 0..w {
            let x_min = x.saturating_sub(r);
            let x_max = (x + r).min(w - 1);

            let mut acc = [0.0f32; 3];
            let mut count = 0.0f32;

            for iy in y_min..=y_max {
                for ix in x_min..=x_max {
                    let idx = (iy * w + ix) * ch;
                    acc[0] += src[idx];
                    acc[1] += src[idx + 1];
                    acc[2] += src[idx + 2];
                    count += 1.0;
                }
            }

            let dst_idx = (y * w + x) * ch;
            dst[dst_idx] = acc[0] / count;
            dst[dst_idx + 1] = acc[1] / count;
            dst[dst_idx + 2] = acc[2] / count;

            if has_alpha {
                dst[dst_idx + ch - 1] = src[dst_idx + ch - 1];
            }
        }
    }
}
