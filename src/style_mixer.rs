#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

const MAX_DEPTH: usize = 64;

impl Json {
    pub fn parse(text: &str) -> Option<Json> {
        let mut p = Parser { b: text.as_bytes(), i: 0 };
        let v = p.value(0)?;
        p.ws();
        (p.i == p.b.len()).then_some(v)
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        use core::fmt::Write;
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Number(n) => {
                let _ = write!(out, "{n}");
            }
            Json::String(s) => write_string(s, out),
            Json::Array(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 { out.push(','); }
                    v.write(out);
                }
                out.push(']');
            }
            Json::Object(fields) => {
                out.push('{');
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 { out.push(','); }
                    write_string(k, out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

fn write_string(s: &str, out: &mut String) {
    use core::fmt::Write;
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => { let _ = write!(out, "\\u{:04x}", c as u32); }
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> { self.b.get(self.i).copied() }
    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) { self.i += 1; }
    }
    fn eat(&mut self, c: u8) -> bool {
        self.ws();
        let hit = self.peek() == Some(c);
        if hit { self.i += 1; }
        hit
    }
    fn literal(&mut self, word: &[u8], value: Json) -> Option<Json> {
        let end = self.i.checked_add(word.len())?;
        (self.b.get(self.i..end)? == word).then(|| { self.i = end; value })
    }
    fn value(&mut self, depth: usize) -> Option<Json> {
        if depth > MAX_DEPTH { return None; }
        self.ws();
        match self.peek()? {
            b'n' => self.literal(b"null", Json::Null),
            b't' => self.literal(b"true", Json::Bool(true)),
            b'f' => self.literal(b"false", Json::Bool(false)),
            b'"' => self.string().map(Json::String),
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                if self.eat(b']') { return Some(Json::Array(items)); }
                loop {
                    items.push(self.value(depth + 1)?);
                    if self.eat(b',') { continue; }
                    return self.eat(b']').then_some(Json::Array(items));
                }
            }
            b'{' => {
            self.i += 1;
            let mut fields = Vec::new();
            if self.eat(b'}') { return Some(Json::Object(fields)); }
            loop {
                self.ws();
                let key = self.string()?;
                if !self.eat(b':') { return None; }
                fields.push((key, self.value(depth + 1)?));
                if self.eat(b',') { continue; }
                return self.eat(b'}').then_some(Json::Object(fields));
            }
            }
            b'-' | b'0'..=b'9' => self.number().map(Json::Number),
            _ => None,
        }
    }
    fn number(&mut self) -> Option<f64> {
        let start = self.i;
        while matches!(self.peek(), Some(b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')) { self.i += 1; }
        let text = core::str::from_utf8(self.b.get(start..self.i)?).ok()?;
        text.parse::<f64>().ok().filter(|n| n.is_finite())
    }
    fn hex4(&mut self) -> Option<u32> {
        let end = self.i.checked_add(4)?;
        let text = core::str::from_utf8(self.b.get(self.i..end)?).ok()?;
        self.i = end;
        u32::from_str_radix(text, 16).ok()
    }
    fn string(&mut self) -> Option<String> {
        if self.peek()? != b'"' { return None; }
        self.i += 1;
        let mut out = Vec::new();
        loop {
            let c = self.peek()?;
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).ok(),
                b'\\' => {
                    let e = self.peek()?;
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"', b'\\' => '\\', b'/' => '/', b'b' => '\u{8}', b'f' => '\u{c}',
                        b'n' => '\n', b'r' => '\r', b't' => '\t',
                        b'u' => {
                            let hi = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&hi) && self.b.get(self.i..self.i + 2) == Some(b"\\u") {
                                self.i += 2;
                                let lo = self.hex4()?;
                                0x10000 + ((hi - 0xD800) << 10) + lo.checked_sub(0xDC00)?
                            } else { hi };
                            char::from_u32(code).unwrap_or('\u{FFFD}')
                        }
                        _ => return None,
                    };
                    let mut tmp = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
                }
                0..=0x1f => return None,
                _ => out.push(c),
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMode {
    Bitmap, Grayscale, Indexed, Rgb, Cmyk, Multichannel, Duotone, Lab, Unknown(u8),
}

impl ColorMode {
    pub fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Bitmap, 1 => Self::Grayscale, 2 => Self::Indexed, 3 => Self::Rgb,
            4 => Self::Cmyk, 7 => Self::Multichannel, 8 => Self::Duotone, 9 => Self::Lab,
            other => Self::Unknown(other),
        }
    }
    pub fn is_rgb(self) -> bool { matches!(self, Self::Rgb | Self::Indexed) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    pub depth: u8,
    pub alpha: bool,
    pub mode: ColorMode,
}

impl Format {
    pub fn from_code(code: u32) -> Self {
        Format { depth: (code & 0xff) as u8, alpha: code & (1 << 8) != 0, mode: ColorMode::from_code(((code >> 16) & 0xff) as u8) }
    }
    pub fn channels(self) -> usize {
        let colour = match self.mode {
            ColorMode::Rgb | ColorMode::Indexed | ColorMode::Lab => 3,
            ColorMode::Cmyk => 4,
            _ => 1,
        };
        colour + usize::from(self.alpha)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub x: i64, pub y: i64, pub overlap: usize, pub canvas_width: usize, pub canvas_height: usize,
}

pub struct Image<'a> {
    pub data: &'a mut [f32],
    pub width: usize,
    pub height: usize,
    pub channels: usize,
    pub format: Format,
    pub geometry: Geometry,
}

pub struct Params { root: Json }

impl Params {
    pub fn new(root: Json) -> Self { Params { root } }
    pub fn parse(text: &str) -> Result<Self, Error> {
        Json::parse(text).filter(|j| matches!(j, Json::Object(_))).map(Params::new).ok_or(Error::BadInput)
    }
    pub fn number(&self, key: &str, default: f64) -> f64 {
        self.root.get(key).and_then(Json::as_f64).unwrap_or(default)
    }
    pub fn text(&self, key: &str) -> Option<&str> {
        self.root.get(key).and_then(Json::as_str)
    }
    pub fn geometry(&self) -> Option<Geometry> {
        let g = self.root.get("_image")?;
        let int = |k: &str| g.get(k).and_then(Json::as_f64).filter(|v| v.is_finite());
        let size = |k: &str| int(k).map(|v| v.clamp(0.0, 1e9) as usize);
        Some(Geometry {
            x: int("x")? as i64, y: int("y")? as i64, overlap: size("overlap").unwrap_or(0),
             canvas_width: size("canvasWidth")?, canvas_height: size("canvasHeight")?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    BadInput = 1, UnsupportedMode = 2, OutOfMemory = 3,
}

pub trait Filter {
    const MANIFEST: &'static str;
    fn apply(image: &mut Image<'_>, params: &Params) -> Result<(), Error>;
}

pub fn try_vec<T: Clone>(len: usize, value: T) -> Result<Vec<T>, Error> {
    let mut v = Vec::new();
    v.try_reserve_exact(len).map_err(|_| Error::OutOfMemory)?;
    v.resize(len, value);
    Ok(v)
}

pub mod abi {
    use super::*;
    pub fn manifest(text: &'static str) -> u64 { ((text.len() as u64) << 32) | (text.as_ptr() as usize as u64) }

    #[cfg(target_arch = "wasm32")]
    pub fn alloc(size: u32) -> u32 {
        let words = (size as usize).div_ceil(8).max(1);
        let mut block: Vec<u64> = Vec::new();
        if block.try_reserve_exact(words).is_err() { return 0; }
        let ptr = block.as_mut_ptr();
        core::mem::forget(block);
        ptr as usize as u32
    }

    #[cfg(target_arch = "wasm32")]
    pub unsafe extern "C" fn filter<F: Filter>(
        buf: *mut f32, len: u32, width: u32, height: u32, channels: u32, format: u32, params: *const u8, params_len: u32,
    ) -> i32 {
        if buf.is_null() || params.is_null() || !len.is_multiple_of(4) || !(buf as usize).is_multiple_of(4) {
            return Error::BadInput as i32;
        }
        let data = unsafe { core::slice::from_raw_parts_mut(buf, len as usize / 4) };
        let params = unsafe { core::slice::from_raw_parts(params, params_len as usize) };
        run::<F>(data, width as usize, height as usize, channels as usize, format, params)
    }

    pub fn run<F: Filter>(data: &mut [f32], width: usize, height: usize, channels: usize, format: u32, params: &[u8]) -> i32 {
        match try_run::<F>(data, width, height, channels, format, params) { Ok(()) => 0, Err(e) => e as i32 }
    }

    pub(crate) fn try_run<F: Filter>(data: &mut [f32], width: usize, height: usize, channels: usize, format: u32, params: &[u8]) -> Result<(), Error> {
        let expected = width.checked_mul(height).and_then(|p| p.checked_mul(channels)).ok_or(Error::BadInput)?;
        if channels == 0 || expected != data.len() { return Err(Error::BadInput); }
        let params = Params::parse(core::str::from_utf8(params).map_err(|_| Error::BadInput)?)?;
        let geometry = params.geometry().unwrap_or(Geometry { x: 0, y: 0, overlap: 0, canvas_width: width, canvas_height: height });
        let mut image = Image { data, width, height, channels, format: Format::from_code(format), geometry };
        F::apply(&mut image, &params)
    }
}

#[macro_export]
macro_rules! export_filter {
    ($filter:ty) => {
        #[cfg(target_arch = "wasm32")]
        pub mod __photocraft_exports {
            use super::*;
            #[unsafe(no_mangle)] pub extern "C" fn pc_abi_version() -> u32 { 1 }
            #[unsafe(no_mangle)] pub extern "C" fn pc_manifest() -> u64 { $crate::abi::manifest(<$filter as $crate::Filter>::MANIFEST) }
            #[unsafe(no_mangle)] pub extern "C" fn pc_alloc(size: u32) -> u32 { $crate::abi::alloc(size) }
            #[unsafe(no_mangle)]
            pub unsafe extern "C" fn pc_filter(
                buf: *mut f32, len: u32, width: u32, height: u32, channels: u32, format: u32, params: *const u8, params_len: u32,
            ) -> i32 {
                unsafe { $crate::abi::filter::<$filter>(buf, len, width, height, channels, format, params, params_len) }
            }
        }
    };
}


// ==========================================
// 2. STYLES & COLOR TRANSFORMATION CORE
// ==========================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CinematicStyle {
    None,
    TealAndOrange,
    BleachBypass,
    Cyberpunk,
    Noir,
    Blockbuster,
}

impl CinematicStyle {
    pub fn from_name(name: &str) -> Self {
        match name {
            "Teal & Orange" => Self::TealAndOrange,
            "Bleach Bypass" => Self::BleachBypass,
            "Cyberpunk" => Self::Cyberpunk,
            "Film Noir" => Self::Noir,
            "Blockbuster" => Self::Blockbuster,
            _ => Self::None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreativeStyle {
    None,
    VibrantPop,
    SunsetGlow,
    EmeraldDream,
    NeonNightmare,
    PastelHaze,
}

impl CreativeStyle {
    pub fn from_name(name: &str) -> Self {
        match name {
            "Vibrant Pop" => Self::VibrantPop,
            "Sunset Glow" => Self::SunsetGlow,
            "Emerald Dream" => Self::EmeraldDream,
            "Neon Nightmare" => Self::NeonNightmare,
            "Pastel Haze" => Self::PastelHaze,
            _ => Self::None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VintageStyle {
    None,
    SepiaClassic,
    Kodachrome64,
    FadedPolaroid,
    Technicolor2Strip,
    CyberRetro,
}

impl VintageStyle {
    pub fn from_name(name: &str) -> Self {
        match name {
            "Sepia Classic" => Self::SepiaClassic,
            "Kodachrome 64" => Self::Kodachrome64,
            "Faded Polaroid" => Self::FadedPolaroid,
            "Technicolor 2-Strip" => Self::Technicolor2Strip,
            "Cyber Retro" => Self::CyberRetro,
            _ => Self::None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtisticStyle {
    None,
    Solarized,
    DuotoneBlueGold,
    HighContrastPunch,
    InfraredDream,
    XproProcess,
}

impl ArtisticStyle {
    pub fn from_name(name: &str) -> Self {
        match name {
            "Solarized" => Self::Solarized,
            "Duotone Blue/Gold" => Self::DuotoneBlueGold,
            "High Contrast Punch" => Self::HighContrastPunch,
            "Infrared Dream" => Self::InfraredDream,
            "Cross Process (Xpro)" => Self::XproProcess,
            _ => Self::None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct StyleSettings {
    pub cinematic: CinematicStyle,
    pub creative: CreativeStyle,
    pub vintage: VintageStyle,
    pub artistic: ArtisticStyle,
    pub intensity: f32,
}

impl StyleSettings {
    pub fn from_params(p: &Params) -> Self {
        let intensity = (p.number("intensity", 100.0) as f32 / 100.0).clamp(0.0, 1.0);
        let cinematic = p.text("cinematicStyle").map(CinematicStyle::from_name).unwrap_or(CinematicStyle::None);
        let creative = p.text("creativeStyle").map(CreativeStyle::from_name).unwrap_or(CreativeStyle::None);
        let vintage = p.text("vintageStyle").map(VintageStyle::from_name).unwrap_or(VintageStyle::None);
        let artistic = p.text("artisticStyle").map(ArtisticStyle::from_name).unwrap_or(ArtisticStyle::None);

        StyleSettings {
            cinematic,
            creative,
            vintage,
            artistic,
            intensity,
        }
    }
}

pub(crate) fn apply_styles(image: &mut Image<'_>, s: &StyleSettings) -> Result<(), Error> {
    let alpha = image.format.alpha;
    let n = image.channels;
    if !image.format.mode.is_rgb() || n != 3 + usize::from(alpha) || image.data.len() != image.width * image.height * n {
        return Err(Error::UnsupportedMode);
    }
    if s.intensity <= 0.0 {
        return Ok(());
    }

    let linear = image.format.depth == 32;

    for chunk in image.data.chunks_exact_mut(n) {
        let (r_in, g_in, b_in) = (chunk[0], chunk[1], chunk[2]);
        // Convert to linear if sRGB encoded for processing
        let mut r = if linear { r_in } else { if r_in <= 0.04045 { r_in / 12.92 } else { ((r_in + 0.055) / 1.055).powf(2.4) } };
        let mut g = if linear { g_in } else { if g_in <= 0.04045 { g_in / 12.92 } else { ((g_in + 0.055) / 1.055).powf(2.4) } };
        let mut b = if linear { b_in } else { if b_in <= 0.04045 { b_in / 12.92 } else { ((b_in + 0.055) / 1.055).powf(2.4) } };

        let (mut tr, mut tg, mut tb) = (r, g, b);

        // Apply Cinematic Styles
        match s.cinematic {
            CinematicStyle::TealAndOrange => {
                let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                tr = r + (0.2 - lum) * 0.3;
                tg = g + (0.1 - lum.abs()) * 0.1;
                tb = b + (lum - 0.2) * 0.3;
            }
            CinematicStyle::BleachBypass => {
                let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                let contrast = (lum - 0.5) * 1.4 + 0.5;
                let desat = (r + g + b) / 3.0;
                tr = r * 0.5 + desat * 0.5 * contrast;
                tg = g * 0.5 + desat * 0.5 * contrast;
                tb = b * 0.5 + desat * 0.5 * contrast;
            }
            CinematicStyle::Cyberpunk => {
                tr = r * 1.2 + 0.05;
                tg = g * 0.8;
                tb = b * 1.4 + 0.1;
            }
            CinematicStyle::Noir => {
                let lum = 0.299 * r + 0.587 * g + 0.114 * b;
                let high_contrast = ((lum - 0.5) * 1.8 + 0.5).clamp(0.0, 1.0);
                tr = high_contrast;
                tg = high_contrast;
                tb = high_contrast;
            }
            CinematicStyle::Blockbuster => {
                tr = r * 1.15;
                tg = g * 0.95;
                tb = b * 1.1;
            }
            CinematicStyle::None => {}
        }

        // Apply Creative Styles
        match s.creative {
            CreativeStyle::VibrantPop => {
                let l = 0.2126 * tr + 0.7152 * tg + 0.0722 * tb;
                tr = l + (tr - l) * 1.35;
                tg = l + (tg - l) * 1.35;
                tb = l + (tb - l) * 1.35;
            }
            CreativeStyle::SunsetGlow => {
                tr *= 1.25;
                tg *= 1.05;
                tb *= 0.85;
            }
            CreativeStyle::EmeraldDream => {
                tr *= 0.85;
                tg *= 1.3;
                tb *= 0.95;
            }
            CreativeStyle::NeonNightmare => {
                tr = (tr * 1.4).sin().abs();
                tg = (tg * 1.2).cos().abs();
                tb = tb * 1.5;
            }
            CreativeStyle::PastelHaze => {
                tr = tr * 0.8 + 0.2;
                tg = tg * 0.8 + 0.2;
                tb = tb * 0.8 + 0.2;
            }
            CreativeStyle::None => {}
        }

        // Apply Vintage Styles
        match s.vintage {
            VintageStyle::SepiaClassic => {
                let l = 0.299 * tr + 0.587 * tg + 0.114 * tb;
                tr = l * 1.2;
                tg = l * 1.0;
                tb = l * 0.8;
            }
            VintageStyle::Kodachrome64 => {
                tr = tr * 1.2 - 0.05;
                tg = tg * 1.1;
                tb = tb * 0.9;
            }
            VintageStyle::FadedPolaroid => {
                let l = 0.2126 * tr + 0.7152 * tg + 0.0722 * tb;
                tr = tr * 0.75 + l * 0.25 + 0.08;
                tg = tg * 0.75 + l * 0.25 + 0.06;
                tb = tb * 0.75 + l * 0.25 + 0.04;
            }
            VintageStyle::Technicolor2Strip => {
                tr = tr * 1.3 - tb * 0.3;
                tg = tg * 0.9;
                tb = tb * 1.2 - tr * 0.2;
            }
            VintageStyle::CyberRetro => {
                tr = tr * 1.25;
                tg = tg * 0.9;
                tb = tb * 1.35;
            }
            VintageStyle::None => {}
        }

        // Apply Artistic Styles
        match s.artistic {
            ArtisticStyle::Solarized => {
                tr = if tr < 0.5 { tr * 2.0 } else { (1.0 - tr) * 2.0 };
                tg = if tg < 0.5 { tg * 2.0 } else { (1.0 - tg) * 2.0 };
                tb = if tb < 0.5 { tb * 2.0 } else { (1.0 - tb) * 2.0 };
            }
            ArtisticStyle::DuotoneBlueGold => {
                let l = 0.2126 * tr + 0.7152 * tg + 0.0722 * tb;
                tr = l * 1.1;
                tg = l * 0.95;
                tb = l * 1.4;
            }
            ArtisticStyle::HighContrastPunch => {
                tr = ((tr - 0.5) * 1.6 + 0.5).clamp(0.0, 1.0);
                tg = ((tg - 0.5) * 1.6 + 0.5).clamp(0.0, 1.0);
                tb = ((tb - 0.5) * 1.6 + 0.5).clamp(0.0, 1.0);
            }
            ArtisticStyle::InfraredDream => {
                let temp = tr;
                tr = tb * 1.3;
                tg = tg * 0.7;
                tb = temp * 1.1;
            }
            ArtisticStyle::XproProcess => {
                tr = tr * 1.3 - 0.1;
                tg = tg * 1.0;
                tb = tb * 0.8 + 0.15;
            }
            ArtisticStyle::None => {}
        }

        // Blend with original according to intensity
        let fi = s.intensity;
        r = r * (1.0 - fi) + tr * fi;
        g = g * (1.0 - fi) + tg * fi;
        b = b * (1.0 - fi) + tb * fi;

        // Convert back to sRGB if needed
        let r_out = if linear { r } else { if r <= 0.0031308 { 12.92 * r } else { 1.055 * r.powf(1.0 / 2.4) - 0.055 } };
        let g_out = if linear { g } else { if g <= 0.0031308 { 12.92 * g } else { 1.055 * g.powf(1.0 / 2.4) - 0.055 } };
        let b_out = if linear { b } else { if b <= 0.0031308 { 12.92 * b } else { 1.055 * b.powf(1.0 / 2.4) - 0.055 } };

        chunk[0] = r_out.clamp(0.0, 1.0);
        chunk[1] = g_out.clamp(0.0, 1.0);
        chunk[2] = b_out.clamp(0.0, 1.0);
    }
    Ok(())
}


// ==========================================
// 3. MANIFEST & PLUGIN DEFINITION
// ==========================================

pub const MANIFEST: &str = concat!(
    r#"{
    "id": "org.photocraft.filter.style_mixer",
    "name": "Procedural Style Mixer",
                                   "version": "1.0.0",
                                   "kind": "filter",
                                   "author": "CHJ85",
                                   "description": "Mix and match different styles.",
                                   "params": {
                                   "cinematicStyle": {
                                   "type": "choice",
                                   "options": ["None", "Teal & Orange", "Bleach Bypass", "Cyberpunk", "Film Noir", "Blockbuster"],
                                   "default": "None"
                                   },
                                   "creativeStyle": {
                                   "type": "choice",
                                   "options": ["None", "Vibrant Pop", "Sunset Glow", "Emerald Dream", "Neon Nightmare", "Pastel Haze"],
                                   "default": "None"
                                   },
                                   "vintageStyle": {
                                   "type": "choice",
                                   "options": ["None", "Sepia Classic", "Kodachrome 64", "Faded Polaroid", "Technicolor 2-Strip", "Cyber Retro"],
                                   "default": "None"
                                   },
                                   "artisticStyle": {
                                   "type": "choice",
                                   "options": ["None", "Solarized", "Duotone Blue/Gold", "High Contrast Punch", "Infrared Dream", "Cross Process (Xpro)"],
                                   "default": "None"
                                   },
                                   "intensity": {
                                   "type": "number",
                                   "min": 0,
                                   "max": 100,
                                   "default": 100
                                   }
                                   },
                                   "overlap": 0,
                                   "area": "canvas"
                                   }"#
);

pub struct StyleTransformerRevisited;

export_filter!(StyleTransformerRevisited);

impl Filter for StyleTransformerRevisited {
    const MANIFEST: &'static str = MANIFEST;

    fn apply(image: &mut Image<'_>, params: &Params) -> Result<(), Error> {
        let settings = StyleSettings::from_params(params);
        apply_styles(image, &settings)
    }
}
