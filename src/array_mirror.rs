#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// 1. Declare UI parameters in the JSON manifest
const MANIFEST: &str = r#"{
"id": "org.photocraft.filter.array_mirror",
"name": "Mirrored Array & Tiling",
"version": "1.0.0",
"kind": "filter",
"author": "CHJ85",
"description": "Creates symmetrical mirrored tiling and array patterns from the image.",
"params": {
"tiles": {
"type": "int",
"default": 2,
"min": 1,
"max": 8,
"label": "Grid Splits"
},
"mirror": {
"type": "number",
"default": 1.0,
"min": 0.0,
"max": 1.0,
"label": "Mirror Symmetry"
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

/// Fast no_std float floor helper
fn floor_f32(val: f32) -> i32 {
    let i = val as i32;
    if (val < 0.0) && (val != i as f32) {
        i - 1
    } else {
        i
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

    let tiles = parse_param_f32(param_bytes, b"tiles", 2.0).max(1.0);
    let mirror_blend = parse_param_f32(param_bytes, b"mirror", 1.0);
    let brightness = parse_param_f32(param_bytes, b"brightness", 1.0);

    // Allocate temporary copy of original pixels to sample from
    let bytes_needed = (total_samples * 4) as u32;
    let src_offset = pc_alloc(bytes_needed);
    if src_offset == 0 {
        return 1;
    }
    let src_ptr = src_offset as *mut f32;
    let src_buf = unsafe { core::slice::from_raw_parts_mut(src_ptr, total_samples) };
    src_buf.copy_from_slice(px);

    // Apply Mirrored Array / Tiling Transformation
    for y in 0..h {
        let fy = (y as f32 / h as f32) * tiles;
        let iy = floor_f32(fy);
        let mut ty = fy - iy as f32;

        // Apply mirror reflection if tile index is odd and mirror symmetry is active
        if iy % 2 != 0 && mirror_blend > 0.0 {
            ty = 1.0 - ty;
        }

        let src_y = ((ty * (h - 1) as f32)).clamp(0.0, (h - 1) as f32) as usize;

        for x in 0..w {
            let fx = (x as f32 / w as f32) * tiles;
            let ix = floor_f32(fx);
            let mut tx = fx - ix as f32;

            if ix % 2 != 0 && mirror_blend > 0.0 {
                tx = 1.0 - tx;
            }

            let src_x = ((tx * (w - 1) as f32)).clamp(0.0, (w - 1) as f32) as usize;

            let dst_idx = (y * w + x) * ch;
            let src_idx = (src_y * w + src_x) * ch;

            if has_alpha && src_buf[src_idx + ch - 1] <= 0.0 {
                continue;
            }

            px[dst_idx] = (src_buf[src_idx] * brightness).clamp(0.0, 1.0);
            px[dst_idx + 1] = (src_buf[src_idx + 1] * brightness).clamp(0.0, 1.0);
            px[dst_idx + 2] = (src_buf[src_idx + 2] * brightness).clamp(0.0, 1.0);

            if has_alpha {
                px[dst_idx + ch - 1] = src_buf[src_idx + ch - 1];
            }
        }
    }

    0
}
