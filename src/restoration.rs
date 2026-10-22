#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// 1. Declare UI parameters in the JSON manifest
const MANIFEST: &str = r#"{
"id": "org.photocraft.filter.restoration",
"name": "Non-Local Means Image Restoration",
"version": "1.0.0",
"kind": "filter",
"author": "CHJ85",
"description": "Miraculous non-AI restoration using patch-based self-similarity to remove noise and preserve sharp detail.",
"params": {
"strength": {
"type": "number",
"default": 0.4,
"min": 0.05,
"max": 1.5,
"label": "Denoise Strength"
},
"search_radius": {
"type": "int",
"default": 2,
"min": 1,
"max": 4,
"label": "Search Window"
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

/// Fast no_std exponential approximation for weight decay calculation
fn exp_approx(x: f32) -> f32 {
    // Fast approximation of exp(-x) for positive x
    let val = 1.0 + x / 8.0;
    let val2 = val * val;
    let val4 = val2 * val2;
    let val8 = val4 * val4;
    1.0 / val8
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

    let strength = parse_param_f32(param_bytes, b"strength", 0.4);
    let search_r_param = parse_param_f32(param_bytes, b"search_radius", 2.0) as usize;
    let brightness = parse_param_f32(param_bytes, b"brightness", 1.0);
    let search_radius = search_r_param.clamp(1, 4);

    // Allocate temp scratch buffer for source reference
    let bytes_needed = (total_samples * 4) as u32;
    let src_offset = pc_alloc(bytes_needed);
    if src_offset == 0 {
        return 1;
    }
    let src_ptr = src_offset as *mut f32;
    let src_buf = unsafe { core::slice::from_raw_parts_mut(src_ptr, total_samples) };
    src_buf.copy_from_slice(px);

    let h_sq = strength * strength * 0.1;

    // Apply Non-Local Means Restoration
    for y in 0..h {
        let y_min = y.saturating_sub(search_radius);
        let y_max = (y + search_radius).min(h - 1);

        for x in 0..w {
            let x_min = x.saturating_sub(search_radius);
            let x_max = (x + search_radius).min(w - 1);
            let dst_idx = (y * w + x) * ch;

            if has_alpha && src_buf[dst_idx + ch - 1] <= 0.0 {
                continue;
            }

            let mut acc_r = 0.0f32;
            let mut acc_g = 0.0f32;
            let mut acc_b = 0.0f32;
            let mut total_weight = 0.0f32;

            let r_ref = src_buf[dst_idx];
            let g_ref = src_buf[dst_idx + 1];
            let b_ref = src_buf[dst_idx + 2];

            // Search neighborhood for self-similar patches
            for ny in y_min..=y_max {
                for nx in x_min..=x_max {
                    let n_idx = (ny * w + nx) * ch;

                    let r_neigh = src_buf[n_idx];
                    let g_neigh = src_buf[n_idx + 1];
                    let b_neigh = src_buf[n_idx + 2];

                    // Patch difference (Sum of Squared Differences)
                    let dr = r_ref - r_neigh;
                    let dg = g_ref - g_neigh;
                    let db = b_ref - b_neigh;
                    let dist_sq = dr * dr + dg * dg + db * db;

                    // Compute exponential weight based on similarity
                    let weight = exp_approx(dist_sq / (h_sq + 0.00001));

                    acc_r += r_neigh * weight;
                    acc_g += g_neigh * weight;
                    acc_b += b_neigh * weight;
                    total_weight += weight;
                }
            }

            if total_weight > 0.0 {
                px[dst_idx] = (acc_r / total_weight * brightness).clamp(0.0, 1.0);
                px[dst_idx + 1] = (acc_g / total_weight * brightness).clamp(0.0, 1.0);
                px[dst_idx + 2] = (acc_b / total_weight * brightness).clamp(0.0, 1.0);
            }

            if has_alpha {
                px[dst_idx + ch - 1] = src_buf[dst_idx + ch - 1];
            }
        }
    }

    0
}
