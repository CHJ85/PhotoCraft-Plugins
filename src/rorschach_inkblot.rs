#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// 1. Declare UI parameters in the JSON manifest
const MANIFEST: &str = r#"{
"id": "org.photocraft.filter.rorschach_inkblot",
"name": "Rorschach Inkblot Generator",
"version": "1.0.0",
"kind": "filter",
"author": "CHJ85",
"description": "Transforms images into symmetrical Rorschach psychological inkblot patterns.",
"params": {
"symmetry_axis": {
"type": "number",
"default": 0.5,
"min": 0.0,
"max": 1.0,
"label": "Axis Offset"
},
"complexity": {
"type": "int",
"default": 3,
"min": 1,
"max": 8,
"label": "Ink Dispersion"
},
"contrast": {
"type": "number",
"default": 1.8,
"min": 0.5,
"max": 3.0,
"label": "Ink Contrast"
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

    let contrast = parse_param_f32(param_bytes, b"contrast", 1.8);
    let complexity_param = parse_param_f32(param_bytes, b"complexity", 3.0) as usize;
    let complexity = complexity_param.clamp(1, 8);

    // Allocate temp scratch buffer for source reference
    let bytes_needed = (total_samples * 4) as u32;
    let src_offset = pc_alloc(bytes_needed);
    if src_offset == 0 {
        return 1;
    }
    let src_ptr = src_offset as *mut f32;
    let src_buf = unsafe { core::slice::from_raw_parts_mut(src_ptr, total_samples) };
    src_buf.copy_from_slice(px);

    let half_w = w / 2;

    // Apply Symmetrical Rorschach Inkblot Transformation
    for y in 0..h {
        for x in 0..half_w {
            let left_idx = (y * w + x) * ch;
            let mirror_x = w - 1 - x;
            let right_idx = (y * w + mirror_x) * ch;

            if has_alpha && src_buf[left_idx + ch - 1] <= 0.0 {
                continue;
            }

            // Sample and blend across the centerline with complexity folding
            let src_l_r = src_buf[left_idx];
            let src_l_g = src_buf[left_idx + 1];
            let src_l_b = src_buf[left_idx + 2];

            let src_r_r = src_buf[right_idx];
            let src_r_g = src_buf[right_idx + 1];
            let src_r_b = src_buf[right_idx + 2];

            let avg_r = (src_l_r + src_r_r) * 0.5;
            let avg_g = (src_l_g + src_r_g) * 0.5;
            let avg_b = (src_l_b + src_r_b) * 0.5;

            let luma = 0.2126 * avg_r + 0.7152 * avg_g + 0.0722 * avg_b;
            let ink = ((1.0 - luma) * contrast * complexity as f32 * 0.3).clamp(0.0, 1.0);
            let ink_val = 1.0 - ink;

            // Apply symmetrical inkblot pixels to both halves
            px[left_idx] = ink_val;
            px[left_idx + 1] = ink_val;
            px[left_idx + 2] = ink_val;

            px[right_idx] = ink_val;
            px[right_idx + 1] = ink_val;
            px[right_idx + 2] = ink_val;

            if has_alpha {
                px[left_idx + ch - 1] = src_buf[left_idx + ch - 1];
                px[right_idx + ch - 1] = src_buf[right_idx + ch - 1];
            }
        }
    }

    0
}
