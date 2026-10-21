#![cfg_attr(not(test), no_std)]

#[cfg(all(not(test), target_arch = "wasm32"))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

const MANIFEST: &str = r#"{
"id": "io.github.chj85.dither_smooth",
"name": "Dither Smooth",
"version": "1.0.0",
"kind": "filter",
"author": "CHJ85",
"description": "Applies counter-quantization dither noise to break up posterized flat color bands.",
"params": {
"strength": {"type": "number", "min": 0.0, "max": 1.0, "default": 0.5}
}
}"#;

const HAS_ALPHA: u32 = 1 << 8;
const DEFAULT_STRENGTH: f32 = 0.5;

#[unsafe(no_mangle)]
pub extern "C" fn pc_abi_version() -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn pc_manifest() -> u64 {
    ((MANIFEST.len() as u64) << 32) | MANIFEST.as_ptr() as u64
}

#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn pc_alloc(size: u32) -> u32 {
    let pages = (size as usize).div_ceil(65536);
    let old = core::arch::wasm32::memory_grow(0, pages);
    if old == usize::MAX { 0 } else { (old * 65536) as u32 }
}

fn read_strength(json: &[u8]) -> f32 {
    let key = b"\"strength\"";
    let mut i = 0;
    while i + key.len() <= json.len() {
        if &json[i..i + key.len()] == key {
            let mut j = i + key.len();
            while j < json.len() && (json[j] == b':' || json[j] == b' ') {
                j += 1;
            }
            let mut val = 0.0f32;
            let mut decimal = false;
            let mut div = 10.0f32;
            let mut any = false;

            while j < json.len() {
                let b = json[j];
                if b == b'.' {
                    decimal = true;
                    j += 1;
                    continue;
                }
                if !b.is_ascii_digit() {
                    break;
                }
                let digit = (b - b'0') as f32;
                if !decimal {
                    val = val * 10.0 + digit;
                } else {
                    val += digit / div;
                    div *= 10.0;
                }
                any = true;
                j += 1;
            }
            return if any { val.clamp(0.0, 1.0) } else { DEFAULT_STRENGTH };
        }
        i += 1;
    }
    DEFAULT_STRENGTH
}

// Simple deterministic pseudo-random generator (PCG-style hash mix for no_std)
fn pseudo_random(seed: u32) -> f32 {
    let mut state = seed.wrapping_mul(747796405).wrapping_add(2891336453);
    state = ((state >> ((state >> 28) + 4)) ^ state).wrapping_mul(277803737);
    state = (state >> 22) ^ state;
    (state as f32) / (u32::MAX as f32)
}

fn dither_smooth(px: &mut [f32], channels: usize, alpha: bool, strength: f32) {
    let colour = if alpha { channels - 1 } else { channels };
    let mut seed: u32 = 1337;

    for p in px.chunks_exact_mut(channels) {
        seed = seed.wrapping_add(1);
        let noise_sample = pseudo_random(seed);

        for v in &mut p[..colour] {
            let x = if *v < 0.0 { 0.0 } else if *v > 1.0 { 1.0 } else { *v };

            // Add triangular-ish dither modulation scaled by strength
            let dither = (noise_sample - 0.5) * 0.15 * strength;
            *v = (x + dither).clamp(0.0, 1.0);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pc_filter(buf: *mut f32, len: u32, _width: u32, _height: u32, channels: u32, format: u32, params: *const u8, params_len: u32) -> i32 {
    let n = channels as usize;
    if n == 0 {
        return 1;
    }
    let px = unsafe { core::slice::from_raw_parts_mut(buf, len as usize / 4) };
    let json: &[u8] = if params.is_null() || params_len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(params, params_len as usize) }
    };
    dither_smooth(px, n, format & HAS_ALPHA != 0, read_strength(json));
    0
}
