#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// 1. Declare UI parameters in the JSON manifest
const MANIFEST: &str = r#"{
"id": "org.photocraft.filter.neon_glow_multicolor",
"name": "Multi-Color Neon Glow FX",
"version": "1.0.0",
"kind": "filter",
"author": "CHJ85",
"description": "Transforms image edges into multi-color glowing neon light vectors with adjustable color palettes.",
"params": {
"intensity": {
"type": "number",
"default": 1.5,
"min": 0.2,
"max": 3.0,
"label": "Glow Intensity"
},
"palette": {
"type": "int",
"default": 0,
"min": 0,
"max": 3,
"label": "Neon Palette (0: Cyan/Magenta, 1: Acid Green/Pink, 2: Cyber Blue/Orange, 3: Fire/Ice)"
},
"brightness": {
"type": "number",
"default": 1.2,
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

    let intensity = parse_param_f32(param_bytes, b"intensity", 1.5);
    let palette = parse_param_f32(param_bytes, b"palette", 0.0) as usize;
    let brightness = parse_param_f32(param_bytes, b"brightness", 1.2);

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

    // Apply Multi-Color Neon Glow Transformation
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

            // Compute local gradient magnitude and direction for multi-color grading
            let dx = luma[y * w + x_next] - luma[y * w + x_prev];
            let dy = luma[y_next * w + x] - luma[y_prev * w + x];
            let grad = sqrt_approx(dx * dx + dy * dy);
            let glow = (grad * intensity * 5.0).clamp(0.0, 1.0);

            // Blend multi-color neon vectors based on coordinate angle and selected palette
            let angle_mix = ((x as f32 / w as f32) + (y as f32 / h as f32)) * 0.5;

            let (r_base, g_base, b_base) = match palette {
                1 => {
                    // Acid Green & Hot Pink
                    (0.1 + angle_mix * 0.9, 1.0 - angle_mix * 0.5, 0.2)
                }
                2 => {
                    // Cyber Blue & Electric Orange
                    (0.2 + angle_mix * 0.8, 0.5 + angle_mix * 0.3, 1.0 - angle_mix * 0.8)
                }
                3 => {
                    // Fire & Ice
                    (1.0, 0.3 + angle_mix * 0.4, 0.2 + (1.0 - angle_mix) * 0.8)
                }
                _ => {
                    // Classic Cyan & Magenta
                    (0.2 + angle_mix * 0.8, 0.9 - angle_mix * 0.4, 1.0)
                }
            };

            px[dst_idx] = (glow * r_base * brightness).clamp(0.0, 1.0);
            px[dst_idx + 1] = (glow * g_base * brightness).clamp(0.0, 1.0);
            px[dst_idx + 2] = (glow * b_base * brightness).clamp(0.0, 1.0);

            if has_alpha {
                px[dst_idx + ch - 1] = src_buf[dst_idx + ch - 1];
            }
        }
    }

    0
}
