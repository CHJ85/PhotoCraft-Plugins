#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// 1. Declare UI parameters in the JSON manifest
const MANIFEST: &str = r#"{
"id": "org.photocraft.filter.comic_book_cel_shading",
"name": "Comic Book & Cel Shading",
"version": "1.0.0",
"kind": "filter",
"author": "CHJ85",
"description": "Transforms images into graphic novel illustrations with bold ink outlines and cel shading.",
"params": {
"outline_weight": {
"type": "number",
"default": 1.2,
"min": 0.2,
"max": 3.0,
"label": "Ink Outline"
},
"color_levels": {
"type": "int",
"default": 4,
"min": 2,
"max": 8,
"label": "Cel Shades"
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

/// Fast no_std floor implementation for f32
fn floor_f32(val: f32) -> f32 {
    let i = val as i32;
    if (i as f32) > val {
        (i - 1) as f32
    } else {
        i as f32
    }
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

    let outline_weight = parse_param_f32(param_bytes, b"outline_weight", 1.2);
    let levels_param = parse_param_f32(param_bytes, b"color_levels", 4.0) as usize;
    let brightness = parse_param_f32(param_bytes, b"brightness", 1.0);
    let levels = levels_param.clamp(2, 8) as f32;

    // Allocate temp scratch buffer for source reference
    let bytes_needed = (total_samples * 4) as u32;
    let src_offset = pc_alloc(bytes_needed);
    if src_offset == 0 {
        return 1;
    }
    let src_ptr = src_offset as *mut f32;
    let src_buf = unsafe { core::slice::from_raw_parts_mut(src_ptr, total_samples) };
    src_buf.copy_from_slice(px);

    // Pre-calculate per-pixel luminance
    let num_pixels = w * h;
    let luma_offset = pc_alloc((num_pixels * 4) as u32);
    if luma_offset == 0 {
        return 1;
    }
    let luma = unsafe { core::slice::from_raw_parts_mut(luma_offset as *mut f32, num_pixels) };

    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * ch;
            luma[y * w + x] = 0.2126 * src_buf[idx] + 0.7152 * src_buf[idx + 1] + 0.0722 * src_buf[idx + 2];
        }
    }

    // Apply Comic Book Inking & Cel Shading
    for y in 0..h {
        let y_prev = y.saturating_sub(1);
        let y_next = (y + 1).min(h - 1);

        for x in 0..w {
            let x_prev = x.saturating_sub(1);
            let x_next = (x + 1).min(w - 1);
            let dst_idx = (y * w + x) * ch;

            if has_alpha && src_buf[dst_idx + ch - 1] <= 0.0 {
                continue;
            }

            // Compute edge gradient for comic ink outlines
            let dx = luma[y * w + x_next] - luma[y * w + x_prev];
            let dy = luma[y_next * w + x] - luma[y_prev * w + x];
            let edge = sqrt_approx(dx * dx + dy * dy) * outline_weight * 4.0;

            if edge > 0.4 {
                // Draw bold ink outline
                px[dst_idx] = 0.05;
                px[dst_idx + 1] = 0.05;
                px[dst_idx + 2] = 0.05;
            } else {
                // Quantize colors into cel-shaded levels using custom floor_f32
                let r = src_buf[dst_idx];
                let g = src_buf[dst_idx + 1];
                let b = src_buf[dst_idx + 2];

                let cel_r = (floor_f32(r * levels) / (levels - 1.0)) * brightness;
                let cel_g = (floor_f32(g * levels) / (levels - 1.0)) * brightness;
                let cel_b = (floor_f32(b * levels) / (levels - 1.0)) * brightness;

                px[dst_idx] = cel_r.clamp(0.0, 1.0);
                px[dst_idx + 1] = cel_g.clamp(0.0, 1.0);
                px[dst_idx + 2] = cel_b.clamp(0.0, 1.0);
            }

            if has_alpha {
                px[dst_idx + ch - 1] = src_buf[dst_idx + ch - 1];
            }
        }
    }

    0
}
