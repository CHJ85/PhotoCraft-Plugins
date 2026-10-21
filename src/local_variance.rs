#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// 1. Declare UI parameters in the JSON manifest
const MANIFEST: &str = r#"{
"id": "org.photocraft.filter.local_variance",
"name": "Local Variance Normalization",
"version": "1.0.0",
"kind": "filter",
"author": "CHJ85",
"description": "Normalizes local contrast to reveal hidden details in extreme shadows and highlights.",
"params": {
"strength": {
"type": "number",
"default": 1.2,
"min": 0.0,
"max": 3.0,
"label": "Contrast Balance"
},
"radius": {
"type": "int",
"default": 3,
"min": 1,
"max": 6,
"label": "Neighborhood Radius"
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

/// Fast no_std square root approximation
fn sqrt_approx(val: f32) -> f32 {
    if val <= 0.0 {
        return 0.0;
    }
    let mut x = val;
    for _ in 0..4 {
        x = 0.5 * (x + val / x);
    }
    x
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

    let strength = parse_param_f32(param_bytes, b"strength", 1.2);
    let r_param = parse_param_f32(param_bytes, b"radius", 3.0) as usize;
    let brightness = parse_param_f32(param_bytes, b"brightness", 1.0);
    let radius = r_param.clamp(1, 6);

    // Allocate temp scratch buffer for source reference
    let bytes_needed = (total_samples * 4) as u32;
    let src_offset = pc_alloc(bytes_needed);
    if src_offset == 0 {
        return 1;
    }
    let src_ptr = src_offset as *mut f32;
    let src_buf = unsafe { core::slice::from_raw_parts_mut(src_ptr, total_samples) };
    src_buf.copy_from_slice(px);

    // Apply Local Variance Normalization
    for y in 0..h {
        let y_min = y.saturating_sub(radius);
        let y_max = (y + radius).min(h - 1);

        for x in 0..w {
            let idx = (y * w + x) * ch;

            if has_alpha && src_buf[idx + ch - 1] <= 0.0 {
                continue;
            }

            let x_min = x.saturating_sub(radius);
            let x_max = (x + radius).min(w - 1);

            // Compute local mean
            let mut acc_r = 0.0f32;
            let mut acc_g = 0.0f32;
            let mut acc_b = 0.0f32;
            let mut count = 0.0f32;

            for iy in y_min..=y_max {
                for ix in x_min..=x_max {
                    let s_idx = (iy * w + ix) * ch;
                    acc_r += src_buf[s_idx];
                    acc_g += src_buf[s_idx + 1];
                    acc_b += src_buf[s_idx + 2];
                    count += 1.0;
                }
            }

            let mean_r = acc_r / count;
            let mean_g = acc_g / count;
            let mean_b = acc_b / count;

            // Compute local variance / standard deviation approximation
            let mut var_r = 0.0f32;
            let mut var_g = 0.0f32;
            let mut var_b = 0.0f32;

            for iy in y_min..=y_max {
                for ix in x_min..=x_max {
                    let s_idx = (iy * w + ix) * ch;
                    let dr = src_buf[s_idx] - mean_r;
                    let dg = src_buf[s_idx + 1] - mean_g;
                    let db = src_buf[s_idx + 2] - mean_b;
                    var_r += dr * dr;
                    var_g += dg * dg;
                    var_b += db * db;
                }
            }

            let std_r = sqrt_approx(var_r / count);
            let std_g = sqrt_approx(var_g / count);
            let std_b = sqrt_approx(var_b / count);

            // Normalize local variance to pull out shadow/highlight details
            let orig_r = src_buf[idx];
            let orig_g = src_buf[idx + 1];
            let orig_b = src_buf[idx + 2];

            let norm_r = mean_r + (orig_r - mean_r) * (strength / (std_r + 0.1));
            let norm_g = mean_g + (orig_g - mean_g) * (strength / (std_g + 0.1));
            let norm_b = mean_b + (orig_b - mean_b) * (strength / (std_b + 0.1));

            px[idx] = (norm_r * brightness).clamp(0.0, 1.0);
            px[idx + 1] = (norm_g * brightness).clamp(0.0, 1.0);
            px[idx + 2] = (norm_b * brightness).clamp(0.0, 1.0);

            if has_alpha {
                px[idx + ch - 1] = src_buf[idx + ch - 1];
            }
        }
    }

    0
}
