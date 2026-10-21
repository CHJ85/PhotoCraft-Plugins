#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// 1. Declare UI parameters in the JSON manifest
const MANIFEST: &str = r#"{
"id": "org.photocraft.filter.comic_book",
"name": "Comic Book & Graphic Novel FX",
"version": "1.0.0",
"kind": "filter",
"author": "CHJ85",
"description": "Converts photos into graphic novel illustration style with ink outlines and color posterization.",
"params": {
"levels": {
"type": "int",
"default": 4,
"min": 2,
"max": 8,
"label": "Color Levels"
},
"edge_weight": {
"type": "number",
"default": 0.8,
"min": 0.0,
"max": 2.0,
"label": "Ink Outline"
},
"saturation": {
"type": "number",
"default": 1.4,
"min": 0.5,
"max": 2.5,
"label": "Color Pop"
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

/// Fast no_std float floor helper
fn floor_f32(val: f32) -> i32 {
    let i = val as i32;
    if (val < 0.0) && (val != i as f32) {
        i - 1
    } else {
        i
    }
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

    let levels = parse_param_f32(param_bytes, b"levels", 4.0).max(2.0);
    let edge_weight = parse_param_f32(param_bytes, b"edge_weight", 0.8);
    let saturation = parse_param_f32(param_bytes, b"saturation", 1.4);

    // Pre-calculate per-pixel luminance for fast edge detection
    let num_pixels = w * h;
    let luma_offset = pc_alloc((num_pixels * 4) as u32);
    if luma_offset == 0 {
        return 1;
    }
    let luma = unsafe { core::slice::from_raw_parts_mut(luma_offset as *mut f32, num_pixels) };

    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * ch;
            luma[y * w + x] = 0.2126 * px[idx] + 0.7152 * px[idx + 1] + 0.0722 * px[idx + 2];
        }
    }

    // Apply Comic Book FX
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * ch;

            if has_alpha && px[idx + ch - 1] <= 0.0 {
                continue;
            }

            // 1. Calculate edge gradient magnitude for ink outlines
            let x_prev = x.saturating_sub(1);
            let x_next = (x + 1).min(w - 1);
            let y_prev = y.saturating_sub(1);
            let y_next = (y + 1).min(h - 1);

            let dx = luma[y * w + x_next] - luma[y * w + x_prev];
            let dy = luma[y_next * w + x] - luma[y_prev * w + x];
            let grad = sqrt_approx(dx * dx + dy * dy);

            let edge_factor = if grad * 3.0 * edge_weight > 0.15 { 0.0 } else { 1.0 };

            // 2. Color saturation boost & Posterization (quantization)
            let r = px[idx];
            let g = px[idx + 1];
            let b = px[idx + 2];

            let l = 0.2126 * r + 0.7152 * g + 0.0722 * b;

            let sr = l + (r - l) * saturation;
            let sg = l + (g - l) * saturation;
            let sb = l + (b - l) * saturation;

            let max_lvl = levels - 1.0;
            let qr = (floor_f32(sr * levels) as f32 / max_lvl).clamp(0.0, 1.0);
            let qg = (floor_f32(sg * levels) as f32 / max_lvl).clamp(0.0, 1.0);
            let qb = (floor_f32(sb * levels) as f32 / max_lvl).clamp(0.0, 1.0);

            // Combine posterized color with ink outline
            px[idx] = (qr * edge_factor).clamp(0.0, 1.0);
            px[idx + 1] = (qg * edge_factor).clamp(0.0, 1.0);
            px[idx + 2] = (qb * edge_factor).clamp(0.0, 1.0);
        }
    }

    0
}
