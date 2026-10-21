#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// 1. Declare UI parameters in the JSON manifest
const MANIFEST: &str = r#"{
"id": "org.photocraft.filter.halftone",
"name": "Halftone Dot Screen FX",
"version": "1.0.0",
"kind": "filter",
"author": "CHJ85",
"description": "Simulates vintage retro print newsprint and comic book halftone dot screens.",
"params": {
"dot_size": {
"type": "int",
"default": 8,
"min": 4,
"max": 24,
"label": "Dot Size"
},
"angle": {
"type": "number",
"default": 45.0,
"min": 0.0,
"max": 90.0,
"label": "Screen Angle"
},
"contrast": {
"type": "number",
"default": 1.5,
"min": 0.5,
"max": 3.0,
"label": "Dot Contrast"
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

/// Sine and cosine approximations for no_std rotation math
fn sin_approx(x: f32) -> f32 {
    let mut val = x % 6.2831853;
    if val < -3.14159265 {
        val += 6.2831853;
    } else if val > 3.14159265 {
        val -= 6.2831853;
    }
    let val2 = val * val;
    val * (1.0 - val2 / 6.0 + val2 * val2 / 120.0)
}

fn cos_approx(x: f32) -> f32 {
    sin_approx(x + 1.57079632)
}

/// Square root approximation for dot distance calculations
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

    let dot_size = parse_param_f32(param_bytes, b"dot_size", 8.0).max(4.0);
    let angle_deg = parse_param_f32(param_bytes, b"angle", 45.0);
    let contrast = parse_param_f32(param_bytes, b"contrast", 1.5);

    let angle_rad = angle_deg * 0.0174532925;
    let cos_a = cos_approx(angle_rad);
    let sin_a = sin_approx(angle_rad);

    // Allocate temp scratch buffer for source reference
    let bytes_needed = (total_samples * 4) as u32;
    let src_offset = pc_alloc(bytes_needed);
    if src_offset == 0 {
        return 1;
    }
    let src_ptr = src_offset as *mut f32;
    let src_buf = unsafe { core::slice::from_raw_parts_mut(src_ptr, total_samples) };
    src_buf.copy_from_slice(px);

    // Apply Halftone Dot Screen Transformation
    for y in 0..h {
        for x in 0..w {
            let dst_idx = (y * w + x) * ch;

            if has_alpha && src_buf[dst_idx + ch - 1] <= 0.0 {
                continue;
            }

            // Rotate coordinates to screen angle
            let rx = x as f32 * cos_a - y as f32 * sin_a;
            let ry = x as f32 * sin_a + y as f32 * cos_a;

            // Find cell center offset
            let cx = (rx % dot_size) - (dot_size * 0.5);
            let cy = (ry % dot_size) - (dot_size * 0.5);
            let dist = sqrt_approx(cx * cx + cy * cy);

            // Sample original luminance
            let r = src_buf[dst_idx];
            let g = src_buf[dst_idx + 1];
            let b = src_buf[dst_idx + 2];
            let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;

            // Halftone radius based on luminance intensity
            let max_radius = dot_size * 0.5 * (((1.0 - luma) * contrast).clamp(0.0, 1.0));
            let dot_color = if dist < max_radius { 0.05 } else { 0.95 };

            px[dst_idx] = dot_color;
            px[dst_idx + 1] = dot_color;
            px[dst_idx + 2] = dot_color;

            if has_alpha {
                px[dst_idx + ch - 1] = src_buf[dst_idx + ch - 1];
            }
        }
    }

    0
}
