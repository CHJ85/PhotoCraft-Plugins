#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// 1. Declare UI parameters in the JSON manifest
const MANIFEST: &str = r#"{
"id": "org.photocraft.filter.crt",
"name": "CRT Scanlines & Sub-Pixels",
"version": "1.0.0",
"kind": "filter",
"author": "CHJ85",
"description": "Emulates vintage CRT scanlines and RGB phosphor aperture masks.",
"params": {
"scanlines": {
"type": "number",
"default": 0.5,
"min": 0.0,
"max": 1.0,
"label": "Scanline Intensity"
},
"phosphor_mask": {
"type": "number",
"default": 0.3,
"min": 0.0,
"max": 1.0,
"label": "Phosphor Mask"
},
"brightness": {
"type": "number",
"default": 1.15,
"min": 0.5,
"max": 2.0,
"label": "Brightness Boost"
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

    let scanline_intensity = parse_param_f32(param_bytes, b"scanlines", 0.5);
    let phosphor_mask = parse_param_f32(param_bytes, b"phosphor_mask", 0.3);
    let brightness = parse_param_f32(param_bytes, b"brightness", 1.15);

    // Apply CRT Filter per pixel
    for y in 0..h {
        // Horizontal scanline darkening factor based on row index
        let scan_factor = if y % 2 == 0 {
            1.0
        } else {
            1.0 - (0.4 * scanline_intensity)
        };

        for x in 0..w {
            let idx = (y * w + x) * ch;

            if has_alpha && px[idx + ch - 1] <= 0.0 {
                continue;
            }

            // RGB Sub-pixel aperture mask pattern across horizontal columns (x % 3)
            let mask_factor = match x % 3 {
                0 => 1.0 - (phosphor_mask * 0.3), // Red sub-pixel column
                1 => 1.0,                         // Green sub-pixel column
                _ => 1.0 - (phosphor_mask * 0.3), // Blue sub-pixel column
            };

            let combined_factor = scan_factor * mask_factor * brightness;

            // Apply to RGB channels
            px[idx] = (px[idx] * combined_factor).clamp(0.0, 1.0);
            px[idx + 1] = (px[idx + 1] * combined_factor).clamp(0.0, 1.0);
            px[idx + 2] = (px[idx + 2] * combined_factor).clamp(0.0, 1.0);
        }
    }

    0
}
