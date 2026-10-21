#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// 1. Declare combined UI parameters in the JSON manifest
const MANIFEST: &str = r#"{
"id": "org.photocraft.filter.crt_bloom",
"name": "CRT Glow & Scanlines",
"version": "1.0.0",
"kind": "filter",
"author": "CHJ85",
"description": "Combines retro CRT scanlines, phosphor triads, and luminance bloom glow.",
"params": {
"scan_intensity": {
"type": "number",
"default": 0.4,
"min": 0.0,
"max": 0.8,
"label": "Scanline Depth"
},
"density": {
"type": "int",
"default": 2,
"min": 1,
"max": 4,
"label": "Scanline Pitch"
},
"threshold": {
"type": "number",
"default": 0.7,
"min": 0.0,
"max": 1.0,
"label": "Bloom Threshold"
},
"radius": {
"type": "int",
"default": 4,
"min": 1,
"max": 16,
"label": "Bloom Radius"
},
"intensity": {
"type": "number",
"default": 1.2,
"min": 0.0,
"max": 4.0,
"label": "Bloom Intensity"
},
"brightness": {
"type": "number",
"default": 1.1,
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

    let scan_intensity = parse_param_f32(param_bytes, b"scan_intensity", 0.4);
    let density_param = parse_param_f32(param_bytes, b"density", 2.0) as usize;
    let threshold = parse_param_f32(param_bytes, b"threshold", 0.7);
    let radius = parse_param_f32(param_bytes, b"radius", 4.0) as usize;
    let intensity = parse_param_f32(param_bytes, b"intensity", 1.2);
    let brightness = parse_param_f32(param_bytes, b"brightness", 1.1);
    let density = density_param.clamp(1, 4);

    // Allocate temp scratch buffers for highlights and blur
    let bytes_needed = (total_samples * 4) as u32;
    let h_offset = pc_alloc(bytes_needed);
    let b_offset = pc_alloc(bytes_needed);

    if h_offset == 0 || b_offset == 0 {
        return 1;
    }

    let highlights = unsafe { core::slice::from_raw_parts_mut(h_offset as *mut f32, total_samples) };
    let blurred = unsafe { core::slice::from_raw_parts_mut(b_offset as *mut f32, total_samples) };

    highlights.fill(0.0);
    blurred.fill(0.0);

    // Step 1: Extract bright highlights above threshold for blooming glow
    for (src_chunk, hl_chunk) in px.chunks_exact(ch).zip(highlights.chunks_exact_mut(ch)) {
        if has_alpha && src_chunk[ch - 1] <= 0.0 {
            continue;
        }

        let luma = 0.2126 * src_chunk[0] + 0.7152 * src_chunk[1] + 0.0722 * src_chunk[2];
        if luma >= threshold {
            hl_chunk[0..3].copy_from_slice(&src_chunk[0..3]);
        }
        if has_alpha {
            hl_chunk[ch - 1] = src_chunk[ch - 1];
        }
    }

    // Step 2: Apply box blur on highlights for the bloom effect
    box_blur(highlights, blurred, w, h, ch, radius, has_alpha);

    // Step 3: Combine Bloom Glow + Original Image + CRT Scanlines & Phosphor Triads
    for y in 0..h {
        let is_scanline = (y / density) % 2 == 0;
        let scan_factor = if is_scanline { 1.0 } else { 1.0 - scan_intensity };

        for x in 0..w {
            let idx = (y * w + x) * ch;

            if has_alpha && px[idx + ch - 1] <= 0.0 {
                continue;
            }

            // Phosphor triad sub-pixel tint
            let triad = match x % 3 {
                0 => 1.05,
                1 => 1.0,
                _ => 0.95,
            };

            // Additive bloom glow
            let bloom_r = blurred[idx];
            let bloom_g = blurred[idx + 1];
            let bloom_b = blurred[idx + 2];

            let r = (px[idx] + bloom_r * intensity).min(1.0);
            let g = (px[idx + 1] + bloom_g * intensity).min(1.0);
            let b = (px[idx + 2] + bloom_b * intensity).min(1.0);

            // Apply CRT scanlines and phosphor triad shading
            px[idx] = (r * scan_factor * triad * brightness).clamp(0.0, 1.0);
            px[idx + 1] = (g * scan_factor * triad * brightness).clamp(0.0, 1.0);
            px[idx + 2] = (b * scan_factor * triad * brightness).clamp(0.0, 1.0);
        }
    }

    0
}

fn box_blur(src: &[f32], dst: &mut [f32], w: usize, h: usize, ch: usize, r: usize, has_alpha: bool) {
    if r == 0 {
        dst.copy_from_slice(src);
        return;
    }

    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 3];
            let mut count = 0.0f32;

            let x_min = x.saturating_sub(r);
            let x_max = (x + r).min(w - 1);

            for ix in x_min..=x_max {
                let idx = (y * w + ix) * ch;
                acc[0] += src[idx];
                acc[1] += src[idx + 1];
                acc[2] += src[idx + 2];
                count += 1.0;
            }

            let idx = (y * w + x) * ch;
            dst[idx] = acc[0] / count;
            dst[idx + 1] = acc[1] / count;
            dst[idx + 2] = acc[2] / count;
            if has_alpha {
                dst[idx + ch - 1] = src[idx + ch - 1];
            }
        }
    }
}
