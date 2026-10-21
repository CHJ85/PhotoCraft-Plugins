#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// 1. Declare UI parameters in the JSON manifest
const MANIFEST: &str = r#"{
"id": "org.photocraft.filter.polygonize",
"name": "Low-Poly Geometric Art",
"version": "1.0.0",
"kind": "filter",
"author": "CHJ85",
"description": "Converts images into low-poly geometric triangular facet artwork.",
"params": {
"facet_size": {
"type": "int",
"default": 16,
"min": 6,
"max": 48,
"label": "Facet Size"
},
"facet_depth": {
"type": "number",
"default": 0.5,
"min": 0.0,
"max": 1.0,
"label": "Shading Depth"
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

    let facet_size = parse_param_f32(param_bytes, b"facet_size", 16.0).max(6.0) as usize;
    let facet_depth = parse_param_f32(param_bytes, b"facet_depth", 0.5);
    let brightness = parse_param_f32(param_bytes, b"brightness", 1.0);

    // Allocate temp scratch buffer for source reference
    let bytes_needed = (total_samples * 4) as u32;
    let src_offset = pc_alloc(bytes_needed);
    if src_offset == 0 {
        return 1;
    }
    let src_ptr = src_offset as *mut f32;
    let src_buf = unsafe { core::slice::from_raw_parts_mut(src_ptr, total_samples) };
    src_buf.copy_from_slice(px);

    // Apply Low-Poly Triangular Facet Tessellation
    let by_max = (h + facet_size - 1) / facet_size;
    let bx_max = (w + facet_size - 1) / facet_size;

    for by in 0..by_max {
        let y_start = by * facet_size;
        let y_end = (y_start + facet_size).min(h);

        for bx in 0..bx_max {
            let x_start = bx * facet_size;
            let x_end = (x_start + facet_size).min(w);

            // Calculate average color for the cell
            let mut acc_r = 0.0f32;
            let mut acc_g = 0.0f32;
            let mut acc_b = 0.0f32;
            let mut count = 0.0f32;

            for y in y_start..y_end {
                for x in x_start..x_end {
                    let s_idx = (y * w + x) * ch;
                    acc_r += src_buf[s_idx];
                    acc_g += src_buf[s_idx + 1];
                    acc_b += src_buf[s_idx + 2];
                    count += 1.0;
                }
            }

            let base_r = (acc_r / count) * brightness;
            let base_g = (acc_g / count) * brightness;
            let base_b = (acc_b / count) * brightness;

            // Tessellate cell into diagonal triangles for low-poly shading facet effect
            for y in y_start..y_end {
                for x in x_start..x_end {
                    let dst_idx = (y * w + x) * ch;

                    if has_alpha && src_buf[dst_idx + ch - 1] <= 0.0 {
                        continue;
                    }

                    // Split cell diagonally to simulate low-poly triangular shading
                    let rel_x = (x - x_start) as f32 / (x_end - x_start).max(1) as f32;
                    let rel_y = (y - y_start) as f32 / (y_end - y_start).max(1) as f32;
                    let is_lower_tri = rel_y > rel_x;

                    let shade = if is_lower_tri {
                        1.0 - (facet_depth * 0.2)
                    } else {
                        1.0 + (facet_depth * 0.2)
                    };

                    px[dst_idx] = (base_r * shade).clamp(0.0, 1.0);
                    px[dst_idx + 1] = (base_g * shade).clamp(0.0, 1.0);
                    px[dst_idx + 2] = (base_b * shade).clamp(0.0, 1.0);

                    if has_alpha {
                        px[dst_idx + ch - 1] = src_buf[dst_idx + ch - 1];
                    }
                }
            }
        }
    }

    0
}
