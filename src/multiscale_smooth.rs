#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// 1. Declare UI parameters in the JSON manifest
const MANIFEST: &str = r#"{
"id": "org.photocraft.filter.multiscale_smooth",
"name": "Edge-Preserving Smoothing",
"version": "1.0.0",
"kind": "filter",
"author": "CHJ85",
"description": "Smooths flat regions and noise while preserving sharp structural edges.",
"params": {
"smoothness": {
"type": "number",
"default": 0.6,
"min": 0.0,
"max": 1.0,
"label": "Smoothness"
},
"radius": {
"type": "int",
"default": 3,
"min": 1,
"max": 6,
"label": "Filter Radius"
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

    let smoothness = parse_param_f32(param_bytes, b"smoothness", 0.6);
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

    // Apply Edge-Preserving Bilateral Smoothing
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

            let center_r = src_buf[idx];
            let center_g = src_buf[idx + 1];
            let center_b = src_buf[idx + 2];

            let mut acc_r = 0.0f32;
            let mut acc_g = 0.0f32;
            let mut acc_b = 0.0f32;
            let mut weight_sum = 0.0f32;

            for iy in y_min..=y_max {
                for ix in x_min..=x_max {
                    let s_idx = (iy * w + ix) * ch;
                    let sr = src_buf[s_idx];
                    let sg = src_buf[s_idx + 1];
                    let sb = src_buf[s_idx + 2];

                    // Color distance for edge preservation
                    let color_diff = (sr - center_r).abs() + (sg - center_g).abs() + (sb - center_b).abs();
                    let edge_weight = 1.0 / (1.0 + color_diff * smoothness * 10.0);

                    acc_r += sr * edge_weight;
                    acc_g += sg * edge_weight;
                    acc_b += sb * edge_weight;
                    weight_sum += edge_weight;
                }
            }

            if weight_sum > 0.0 {
                px[idx] = ((acc_r / weight_sum) * brightness).clamp(0.0, 1.0);
                px[idx + 1] = ((acc_g / weight_sum) * brightness).clamp(0.0, 1.0);
                px[idx + 2] = ((acc_b / weight_sum) * brightness).clamp(0.0, 1.0);
            }

            if has_alpha {
                px[idx + ch - 1] = src_buf[idx + ch - 1];
            }
        }
    }

    0
}
