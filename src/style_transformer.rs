#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// 1. Declare UI parameters in the JSON manifest with a locked range (0 to 51)
const MANIFEST: &str = r#"{
"id": "org.photocraft.filter.style_transformer",
"name": "Procedural Style Transformer",
"version": "3.0.0",
"kind": "filter",
"author": "CHJ85",
"description": "Procedurally transforms images across 50 distinct artistic, film, and mood styles (0 & 51 are default).",
"params": {
"style": {
"type": "int",
"default": 0,
"min": 0,
"max": 51,
"label": "Style (0/51: Default, 1-50: Unique Artistic & Film Styles)"
},
"strength": {
"type": "number",
"default": 1.0,
"min": 0.0,
"max": 1.0,
"label": "Style Blend"
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

    let style_param = parse_param_f32(param_bytes, b"style", 0.0) as usize;
    let strength = parse_param_f32(param_bytes, b"strength", 1.0);
    let brightness = parse_param_f32(param_bytes, b"brightness", 1.0);
    let style = style_param.clamp(0, 51);

    // Allocate temp scratch buffer for source reference
    let bytes_needed = (total_samples * 4) as u32;
    let src_offset = pc_alloc(bytes_needed);
    if src_offset == 0 {
        return 1;
    }
    let src_ptr = src_offset as *mut f32;
    let src_buf = unsafe { core::slice::from_raw_parts_mut(src_ptr, total_samples) };
    src_buf.copy_from_slice(px);

    // Apply procedural style palette transformations across 50 distinct styles
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * ch;

            if has_alpha && src_buf[idx + ch - 1] <= 0.0 {
                continue;
            }

            let sr = src_buf[idx];
            let sg = src_buf[idx + 1];
            let sb = src_buf[idx + 2];
            let luma = 0.2126 * sr + 0.7152 * sg + 0.0722 * sb;

            let (tr, tg, tb) = match style {
                0 | 51 => {
                    // 0 & 51: Default / Unfiltered Original Image
                    (sr, sg, sb)
                }
                1 => {
                    // 1: Anime Style
                    ((sr * 1.2).clamp(0.0, 1.0), (sg * 1.1).clamp(0.0, 1.0), (sb * 1.3 + 0.1).clamp(0.0, 1.0))
                }
                2 => {
                    // 2: Cartoon Style
                    (
                        (floor_f32(sr * 4.0) / 3.0).clamp(0.0, 1.0),
                     (floor_f32(sg * 4.0) / 3.0).clamp(0.0, 1.0),
                     (floor_f32(sb * 4.0) / 3.0).clamp(0.0, 1.0),
                    )
                }
                3 => {
                    // 3: Cyberpunk Style
                    let r = (sr * 1.4 + 0.1).clamp(0.0, 1.0);
                    let g = (sg * 0.9).clamp(0.0, 1.0);
                    let b = (sb * 1.6 + 0.2).clamp(0.0, 1.0);
                    (r * (luma * 0.8 + 0.2), g * (luma * 0.8 + 0.2), b * (luma * 0.8 + 0.2))
                }
                4 => {
                    // 4: Cinematic Style
                    ((sr * sr * 1.2).clamp(0.0, 1.0), (sg * sg * 1.1).clamp(0.0, 1.0), (sb * sb * 0.9 + 0.05).clamp(0.0, 1.0))
                }
                5 => {
                    // 5: Noir Style
                    (luma * luma * 1.3, luma * luma * 1.3, luma * luma * 1.3)
                }
                6 => {
                    // 6: Sepia Style
                    ((luma * 1.2).clamp(0.0, 1.0), (luma * 0.95).clamp(0.0, 1.0), (luma * 0.75).clamp(0.0, 1.0))
                }
                7 => {
                    // 7: Technicolor Style
                    ((sr * 1.4 - 0.1).clamp(0.0, 1.0), (sg * 1.2).clamp(0.0, 1.0), (sb * 1.3).clamp(0.0, 1.0))
                }
                8 => {
                    // 8: Vaporwave Style
                    ((sr * 1.2 + sb * 0.4).clamp(0.0, 1.0), (sg * 0.7).clamp(0.0, 1.0), (sb * 1.5 + 0.2).clamp(0.0, 1.0))
                }
                9 => {
                    // 9: Bleach Bypass
                    let blend = luma * 1.2;
                    ((sr * blend * 1.5).clamp(0.0, 1.0), (sg * blend * 1.3).clamp(0.0, 1.0), (sb * blend * 1.2).clamp(0.0, 1.0))
                }
                10 => {
                    // 10: Thermal Infrared
                    let t = luma;
                    let r = (t * 3.0).clamp(0.0, 1.0);
                    let g = ((t - 0.33) * 3.0).clamp(0.0, 1.0);
                    let b = ((t - 0.66) * 3.0).clamp(0.0, 1.0);
                    (r, g, b)
                }
                11 => {
                    // 11: X-Ray Inverted
                    (1.0 - sr, 1.0 - sg, 1.0 - sb)
                }
                12 => {
                    // 12: Golden Hour
                    ((sr * 1.3 + 0.1).clamp(0.0, 1.0), (sg * 1.1 + 0.05).clamp(0.0, 1.0), (sb * 0.8).clamp(0.0, 1.0))
                }
                13 => {
                    // 13: Arctic Frost
                    ((sr * 0.8).clamp(0.0, 1.0), (sg * 1.05).clamp(0.0, 1.0), (sb * 1.4 + 0.15).clamp(0.0, 1.0))
                }
                14 => {
                    // 14: Emerald Forest
                    ((sr * 0.85).clamp(0.0, 1.0), (sg * 1.35 + 0.05).clamp(0.0, 1.0), (sb * 0.8).clamp(0.0, 1.0))
                }
                15 => {
                    // 15: Retro Polaroid
                    ((sr * 1.1 + 0.1).clamp(0.0, 1.0), (sg * 1.05 + 0.05).clamp(0.0, 1.0), (sb * 0.95 + 0.1).clamp(0.0, 1.0))
                }
                16 => {
                    // 16: Post-Apocalyptic
                    ((sr * 1.3 + 0.15).clamp(0.0, 1.0), (sg * 1.1).clamp(0.0, 1.0), (sb * 0.6).clamp(0.0, 1.0))
                }
                17 => {
                    // 17: Synthwave Style
                    ((sr * 1.5).clamp(0.0, 1.0), (sg * 0.6).clamp(0.0, 1.0), (sb * 1.6 + 0.2).clamp(0.0, 1.0))
                }
                18 => {
                    // 18: High-Key
                    ((sr * 0.7 + 0.3).clamp(0.0, 1.0), (sg * 0.7 + 0.3).clamp(0.0, 1.0), (sb * 0.7 + 0.3).clamp(0.0, 1.0))
                }
                19 => {
                    // 19: Low-Key
                    ((sr * sr * sr * 1.2).clamp(0.0, 1.0), (sg * sg * sg * 1.2).clamp(0.0, 1.0), (sb * sb * sb * 1.2).clamp(0.0, 1.0))
                }
                20 => {
                    // 20: Emerald Noir
                    (luma * 0.6, luma * 1.2, luma * 0.7)
                }
                21 => {
                    // 21: Crimson Horror
                    (luma * 1.4, luma * 0.2, luma * 0.2)
                }
                22 => {
                    // 22: Cobalt Blue
                    (luma * 0.2, luma * 0.5, luma * 1.5)
                }
                23 => {
                    // 23: Amber CRT
                    (luma * 1.3, luma * 0.9, luma * 0.2)
                }
                24 => {
                    // 24: Solarized
                    (
                        if sr > 0.5 { 1.0 - sr } else { sr },
                            if sg > 0.5 { 1.0 - sg } else { sg },
                                if sb > 0.5 { 1.0 - sb } else { sb },
                    )
                }
                25 => {
                    // 25: Duotone Cyan/Yellow
                    (luma, luma * 1.2, luma * 1.4)
                }
                26 => {
                    // 26: Duotone Magenta/Green
                    (luma * 1.3, luma * 1.1, luma * 1.3)
                }
                27 => {
                    // 27: Cross-Processed
                    (sr * 1.2, sg * 0.8, sb * 1.3)
                }
                28 => {
                    // 28: Pastel Dream
                    (sr * 0.8 + 0.2, sg * 0.8 + 0.2, sb * 0.9 + 0.1)
                }
                29 => {
                    // 29: Cyber Blue
                    (sr * 0.7, sg * 1.1, sb * 1.6)
                }
                30 => {
                    // 30: Sunset Glow
                    (sr * 1.4, sg * 0.9, sb * 1.2)
                }
                31 => {
                    // 31: Deep Space
                    (sr * 0.5, sg * 0.5, sb * 1.4)
                }
                32 => {
                    // 32: Vintage Postcard
                    (sr * 1.1 + 0.1, sg * 1.0, sb * 0.85)
                }
                33 => {
                    // 33: Acid Trip
                    (1.0 - sr, 1.0 - sg, sb * 1.5)
                }
                34 => {
                    // 34: Matrix Green
                    (luma * 0.2, luma * 1.4, luma * 0.2)
                }
                35 => {
                    // 35: Neon Pink
                    (sr * 1.5, sg * 0.4, sb * 1.4)
                }
                36 => {
                    // 36: Arctic Blue
                    (sr * 0.8, sg * 1.2, sb * 1.5)
                }
                37 => {
                    // 37: Copper Tone
                    (sr * 1.3, sg * 0.9, sb * 0.6)
                }
                38 => {
                    // 38: Midnight Blue
                    (sr * 0.3, sg * 0.5, sb * 1.2)
                }
                39 => {
                    // 39: Technicolor Two-Strip
                    (sr * 1.4, sg * 1.3, sb * 0.5)
                }
                40 => {
                    // 40: Infravision
                    (luma * 1.5, luma * 0.4, luma * 0.6)
                }
                41 => {
                    // 41: Solar Flare
                    (sr * 1.5, sg * 1.3, sb * 0.7)
                }
                42 => {
                    // 42: Toxic Waste
                    (sr * 0.5, sg * 1.6, sb * 0.3)
                }
                43 => {
                    // 43: Steel Grayscale
                    (luma * 0.9, luma * 1.0, luma * 1.2)
                }
                44 => {
                    // 44: Chocolate Box
                    (sr * 1.2, sg * 0.8, sb * 0.5)
                }
                45 => {
                    // 45: Plum Noir
                    (sr * 1.1, sg * 0.5, sb * 1.2)
                }
                46 => {
                    // 46: Arctic Dawn
                    (sr * 0.9, sg * 1.1, sb * 1.4)
                }
                47 => {
                    // 47: Retro 70s
                    (sr * 1.2, sg * 1.0, sb * 0.7)
                }
                48 => {
                    // 48: Cyberpunk Gold
                    (sr * 1.4, sg * 1.2, sb * 0.4)
                }
                49 => {
                    // 49: Hyper-Saturation
                    (sr * sr * 1.5, sg * sg * 1.5, sb * sb * 1.5)
                }
                _ => {
                    // 50: Charcoal Sketch Tone
                    (1.0 - luma * luma, 1.0 - luma * luma, 1.0 - luma * luma)
                }
            };

            // Blend with original based on 'strength' slider
            let final_r = (sr * (1.0 - strength) + tr * strength) * brightness;
            let final_g = (sg * (1.0 - strength) + tg * strength) * brightness;
            let final_b = (sb * (1.0 - strength) + tb * strength) * brightness;

            px[idx] = final_r.clamp(0.0, 1.0);
            px[idx + 1] = final_g.clamp(0.0, 1.0);
            px[idx + 2] = final_b.clamp(0.0, 1.0);

            if has_alpha {
                px[idx + ch - 1] = src_buf[idx + ch - 1];
            }
        }
    }

    0
}
