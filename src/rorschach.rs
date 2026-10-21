#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// 1. Declare UI parameters in the JSON manifest
const MANIFEST: &str = r#"{
"id": "org.photocraft.filter.rorschach",
"name": "Rorschach Inkblot FX",
"version": "1.0.0",
"kind": "filter",
"author": "CHJ85",
"description": "Generates symmetrical Rorschach psychological inkblot patterns from images.",
"params": {
"symmetry": {
"type": "number",
"default": 1.0,
"min": 0.0,
"max": 1.0,
"label": "Mirror Symmetry"
},
"contrast": {
"type": "number",
"default": 1.8,
"min": 0.5,
"max": 3.5,
"label": "Ink Contrast"
},
"threshold": {
"type": "number",
"default": 0.4,
"min": 0.0,
"max": 1.0,
"label": "Ink Density"
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

    let symmetry = parse_param_f32(param_bytes, b"symmetry", 1.0);
    let contrast = parse_param_f32(param_bytes, b"contrast", 1.8);
    let threshold = parse_param_f32(param_bytes, b"threshold", 0.4);

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

    // Apply Rorschach Inkblot Transformation
    for y in 0..h {
        for x in 0..w {
            let dst_idx = (y * w + x) * ch;

            if has_alpha && src_buf[dst_idx + ch - 1] <= 0.0 {
                continue;
            }

            // Mirror coordinates across the vertical center axis if symmetry is active
            let sample_x = if symmetry > 0.0 && x > half_w {
                w - 1 - x
            } else {
                x
            };

            let src_idx = (y * w + sample_x) * ch;

            // Convert to luminance and apply ink contrast curve
            let r = src_buf[src_idx];
            let g = src_buf[src_idx + 1];
            let b = src_buf[src_idx + 2];

            let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            let adjusted = (((luma - threshold) * contrast) + 0.5).clamp(0.0, 1.0);

            // Output stylized dark ink tones
            px[dst_idx] = adjusted;
            px[dst_idx + 1] = adjusted;
            px[dst_idx + 2] = adjusted;

            if has_alpha {
                px[dst_idx + ch - 1] = src_buf[src_idx + ch - 1];
            }
        }
    }

    0
}
