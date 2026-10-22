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
// 2. MOTION PICTURES XL ENUMS & PIPELINE
// ==========================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VintageStyle { None, SepiaClassic, FadedPolaroid, Retro70s, CyberRetro, GrungeRock, VintageFilm, GoldenEra, AntiqueWarm, RetroFade }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BnWStyle { None, CinematicNoir, HighContrastMono, SilverGelatin, MonochromeGold, DarkMoodyMono, ClassicMono, NoirContrast, SoftSilver, AgedMono }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilmStyle { None, Kodachrome64, Technicolor2Strip, StandardFilmStock, BleachBypass, CrossProcess, CineTone, ArriLook, RedEpicStyle, Technicolor3Strip }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CinemaStyle { None, TealAndOrange, Blockbuster, DarkThriller, IndieDrama, EpicAdventure, HollywoodGold, ActionBlockbuster, SciFiBlue, NeoNoir }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoodyStyle { None, MysteryTone, AlpineChill, DeepForest, AutumnHaze, ArcticFrost, SomberGray, StormyBlue, TwilightGlow, FoggyHaze }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstantStyle { None, PolaroidExpired, InstaxWarm, PolaroidVivid, ToyCamera, LofiSnap, InstaxCool, PolaroidFade, PolaroidVintage, InstantClassic }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlideStyle { None, EktachromeWarm, VelviaVivid, AgfaSlide, KodachromeSlide, HighSatSlide, SlidePositive, FujiSlide, RichSlide, WarmSlide }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FujifilmStyle { None, ClassicChrome, AstiaSoft, ProviaStandard, AcrosMono, ClassicNeg, SuperiaFilm, EternaCinema, VelviaFilm, ProNegHi }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NegativeStyle { None, PortraWarm, SuperiaVivid, GoldConsumer, CinestillHalation, UltramaxPop, ColorNeg100, ColorNeg400, WarmNegative, SoftNegative }

#[derive(Clone, Copy, Debug)]
pub struct MotionSettings {
    pub vintage: VintageStyle, pub v_press: f32,
    pub bnw: BnWStyle, pub b_press: f32,
    pub film: FilmStyle, pub f_press: f32,
    pub cinema: CinemaStyle, pub c_press: f32,
    pub moody: MoodyStyle, pub m_press: f32,
    pub instant: InstantStyle, pub i_press: f32,
    pub slide: SlideStyle, pub s_press: f32,
    pub fujifilm: FujifilmStyle, pub fj_press: f32,
    pub negative: NegativeStyle, pub n_press: f32,
}

impl MotionSettings {
    pub fn from_params(p: &Params) -> Self {
        let pp = |k: &str| (p.number(k, 100.0) as f32 / 100.0).clamp(0.0, 1.0);
        Self {
            vintage: match p.text("vintageStyle") {
                Some("Sepia Classic") => VintageStyle::SepiaClassic, Some("Faded Polaroid") => VintageStyle::FadedPolaroid,
                Some("Retro 70s") => VintageStyle::Retro70s, Some("Cyber Retro") => VintageStyle::CyberRetro,
                Some("Grunge Rock") => VintageStyle::GrungeRock, Some("Vintage Film") => VintageStyle::VintageFilm,
                Some("Golden Era") => VintageStyle::GoldenEra, Some("Antique Warm") => VintageStyle::AntiqueWarm,
                Some("Retro Fade") => VintageStyle::RetroFade, _ => VintageStyle::None,
            }, v_press: pp("vintagePressure"),
            bnw: match p.text("bnwStyle") {
                Some("Cinematic Noir") => BnWStyle::CinematicNoir, Some("High Contrast Mono") => BnWStyle::HighContrastMono,
                Some("Silver Gelatin") => BnWStyle::SilverGelatin, Some("Monochrome Gold") => BnWStyle::MonochromeGold,
                Some("Dark Moody Mono") => BnWStyle::DarkMoodyMono, Some("Classic Mono") => BnWStyle::ClassicMono,
                Some("Noir Contrast") => BnWStyle::NoirContrast, Some("Soft Silver") => BnWStyle::SoftSilver,
                Some("Aged Mono") => BnWStyle::AgedMono, _ => BnWStyle::None,
            }, b_press: pp("bnwPressure"),
            film: match p.text("filmStyle") {
                Some("Kodachrome 64") => FilmStyle::Kodachrome64, Some("Technicolor 2-Strip") => FilmStyle::Technicolor2Strip,
                Some("Standard Film Stock") => FilmStyle::StandardFilmStock, Some("Bleach Bypass") => FilmStyle::BleachBypass,
                Some("Cross Process (Xpro)") => FilmStyle::CrossProcess, Some("CineTone") => FilmStyle::CineTone,
                Some("Arri Look") => FilmStyle::ArriLook, Some("Red Epic Style") => FilmStyle::RedEpicStyle,
                Some("Technicolor 3-Strip") => FilmStyle::Technicolor3Strip, _ => FilmStyle::None,
            }, f_press: pp("filmPressure"),
            cinema: match p.text("cinemaStyle") {
                Some("Teal & Orange") => CinemaStyle::TealAndOrange, Some("Blockbuster") => CinemaStyle::Blockbuster,
                Some("Dark Thriller") => CinemaStyle::DarkThriller, Some("Indie Drama") => CinemaStyle::IndieDrama,
                Some("Epic Adventure") => CinemaStyle::EpicAdventure, Some("Hollywood Gold") => CinemaStyle::HollywoodGold,
                Some("Action Blockbuster") => CinemaStyle::ActionBlockbuster, Some("Sci-Fi Blue") => CinemaStyle::SciFiBlue,
                Some("Neo-Noir") => CinemaStyle::NeoNoir, _ => CinemaStyle::None,
            }, c_press: pp("cinemaPressure"),
            moody: match p.text("moodyStyle") {
                Some("Mystery Tone") => MoodyStyle::MysteryTone, Some("Alpine Chill") => MoodyStyle::AlpineChill,
                Some("Deep Forest") => MoodyStyle::DeepForest, Some("Autumn Haze") => MoodyStyle::AutumnHaze,
                Some("Arctic Frost") => MoodyStyle::ArcticFrost, Some("Somber Gray") => MoodyStyle::SomberGray,
                Some("Stormy Blue") => MoodyStyle::StormyBlue, Some("Twilight Glow") => MoodyStyle::TwilightGlow,
                Some("Foggy Haze") => MoodyStyle::FoggyHaze, _ => MoodyStyle::None,
            }, m_press: pp("moodyPressure"),
            instant: match p.text("instantStyle") {
                Some("Polaroid Expired") => InstantStyle::PolaroidExpired, Some("Instax Warm") => InstantStyle::InstaxWarm,
                Some("Polaroid Vivid") => InstantStyle::PolaroidVivid, Some("Toy Camera") => InstantStyle::ToyCamera,
                Some("Lo-Fi Snap") => InstantStyle::LofiSnap, Some("Instax Cool") => InstantStyle::InstaxCool,
                Some("Polaroid Fade") => InstantStyle::PolaroidFade, Some("Polaroid Vintage") => InstantStyle::PolaroidVintage,
                Some("Instant Classic") => InstantStyle::InstantClassic, _ => InstantStyle::None,
            }, i_press: pp("instantPressure"),
            slide: match p.text("slideStyle") {
                Some("Ektachrome Warm") => SlideStyle::EktachromeWarm, Some("Velvia Vivid") => SlideStyle::VelviaVivid,
                Some("Agfa Slide") => SlideStyle::AgfaSlide, Some("Kodachrome Slide") => SlideStyle::KodachromeSlide,
                Some("High Sat Slide") => SlideStyle::HighSatSlide, Some("Slide Positive") => SlideStyle::SlidePositive,
                Some("Fuji Slide") => SlideStyle::FujiSlide, Some("Rich Slide") => SlideStyle::RichSlide,
                Some("Warm Slide") => SlideStyle::WarmSlide, _ => SlideStyle::None,
            }, s_press: pp("slidePressure"),
            fujifilm: match p.text("fujifilmStyle") {
                Some("Classic Chrome") => FujifilmStyle::ClassicChrome, Some("Astia Soft") => FujifilmStyle::AstiaSoft,
                Some("Provia Standard") => FujifilmStyle::ProviaStandard, Some("Acros Mono") => FujifilmStyle::AcrosMono,
                Some("Classic Neg") => FujifilmStyle::ClassicNeg, Some("Superia Film") => FujifilmStyle::SuperiaFilm,
                Some("Eterna Cinema") => FujifilmStyle::EternaCinema, Some("Velvia Film") => FujifilmStyle::VelviaFilm,
                Some("Pro Neg Hi") => FujifilmStyle::ProNegHi, _ => FujifilmStyle::None,
            }, fj_press: pp("fujifilmPressure"),
            negative: match p.text("negativeStyle") {
                Some("Portra Warm") => NegativeStyle::PortraWarm, Some("Superia Vivid") => NegativeStyle::SuperiaVivid,
                Some("Gold Consumer") => NegativeStyle::GoldConsumer, Some("Cinestill Halation") => NegativeStyle::CinestillHalation,
                Some("Ultramax Pop") => NegativeStyle::UltramaxPop, Some("Color Neg 100") => NegativeStyle::ColorNeg100,
                Some("Color Neg 400") => NegativeStyle::ColorNeg400, Some("Warm Negative") => NegativeStyle::WarmNegative,
                Some("Soft Negative") => NegativeStyle::SoftNegative, _ => NegativeStyle::None,
            }, n_press: pp("negativePressure"),
        }
    }
}

pub(crate) fn apply_motion_pictures(image: &mut Image<'_>, s: &MotionSettings) -> Result<(), Error> {
    let alpha = image.format.alpha;
    let n = image.channels;
    if !image.format.mode.is_rgb() || n != 3 + usize::from(alpha) || image.data.len() != image.width * image.height * n {
        return Err(Error::UnsupportedMode);
    }

    let linear = image.format.depth == 32;

    for chunk in image.data.chunks_exact_mut(n) {
        let (r_in, g_in, b_in) = (chunk[0], chunk[1], chunk[2]);
        let mut r = if linear { r_in } else { if r_in <= 0.04045 { r_in / 12.92 } else { ((r_in + 0.055) / 1.055).powf(2.4) } };
        let mut g = if linear { g_in } else { if g_in <= 0.04045 { g_in / 12.92 } else { ((g_in + 0.055) / 1.055).powf(2.4) } };
        let mut b = if linear { b_in } else { if b_in <= 0.04045 { b_in / 12.92 } else { ((b_in + 0.055) / 1.055).powf(2.4) } };

        // 1. Vintage Pipeline
        if s.vintage != VintageStyle::None && s.v_press > 0.0 {
            let (mut tr, mut tg, mut tb) = (r, g, b);
            match s.vintage {
                VintageStyle::SepiaClassic => { let l = 0.299 * r + 0.587 * g + 0.114 * b; tr = l * 1.2; tg = l * 1.0; tb = l * 0.8; }
                VintageStyle::FadedPolaroid => { let l = 0.2126 * r + 0.7152 * g + 0.0722 * b; tr = r * 0.75 + l * 0.25 + 0.08; tg = g * 0.75 + l * 0.25 + 0.06; tb = b * 0.75 + l * 0.25 + 0.04; }
                VintageStyle::Retro70s => { tr = r * 1.15; tg = g * 1.05; tb = b * 0.75; }
                VintageStyle::CyberRetro => { tr = r * 1.25; tg = g * 0.9; tb = b * 1.35; }
                VintageStyle::GrungeRock => { tr = ((r - 0.5) * 1.5 + 0.5) * 0.9; tg = ((g - 0.5) * 1.5 + 0.5) * 0.85; tb = ((b - 0.5) * 1.5 + 0.5) * 0.8; }
                VintageStyle::VintageFilm => { tr = r * 1.1; tg = g * 0.95; tb = b * 0.85; }
                VintageStyle::GoldenEra => { tr = r * 1.2; tg = g * 1.1; tb = b * 0.8; }
                VintageStyle::AntiqueWarm => { tr = r * 1.15; tg = g * 1.02; tb = b * 0.9; }
                VintageStyle::RetroFade => { tr = r * 0.9 + 0.1; tg = g * 0.9 + 0.1; tb = b * 0.9 + 0.05; }
                VintageStyle::None => {}
            }
            let p = s.v_press; r = r * (1.0 - p) + tr * p; g = g * (1.0 - p) + tg * p; b = b * (1.0 - p) + tb * p;
        }

        // 2. Black & White Pipeline
        if s.bnw != BnWStyle::None && s.b_press > 0.0 {
            let (mut tr, mut tg, mut tb) = (r, g, b);
            match s.bnw {
                BnWStyle::CinematicNoir => { let lum = 0.299 * r + 0.587 * g + 0.114 * b; let hc = ((lum - 0.5) * 1.8 + 0.5).clamp(0.0, 1.0); tr = hc; tg = hc; tb = hc; }
                BnWStyle::HighContrastMono => { let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b; let hc = ((lum - 0.5) * 2.0 + 0.5).clamp(0.0, 1.0); tr = hc; tg = hc; tb = hc; }
                BnWStyle::SilverGelatin => { let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b; tr = lum * 1.05; tg = lum * 1.05; tb = lum * 1.05; }
                BnWStyle::MonochromeGold => { let lum = 0.299 * r + 0.587 * g + 0.114 * b; tr = lum * 1.3; tg = lum * 1.1; tb = lum * 0.7; }
                BnWStyle::DarkMoodyMono => { let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b; let dark = (lum * 0.8).clamp(0.0, 1.0); tr = dark; tg = dark; tb = dark; }
                BnWStyle::ClassicMono => { let lum = 0.299 * r + 0.587 * g + 0.114 * b; tr = lum; tg = lum; tb = lum; }
                BnWStyle::NoirContrast => { let lum = 0.299 * r + 0.587 * g + 0.114 * b; let nc = ((lum - 0.5) * 2.2 + 0.5).clamp(0.0, 1.0); tr = nc; tg = nc; tb = nc; }
                BnWStyle::SoftSilver => { let lum = 0.299 * r + 0.587 * g + 0.114 * b; tr = lum * 0.9 + 0.05; tg = lum * 0.9 + 0.05; tb = lum * 0.9 + 0.05; }
                BnWStyle::AgedMono => { let lum = 0.299 * r + 0.587 * g + 0.114 * b; tr = lum * 1.1; tg = lum * 1.05; tb = lum * 0.9; }
                BnWStyle::None => {}
            }
            let p = s.b_press; r = r * (1.0 - p) + tr * p; g = g * (1.0 - p) + tg * p; b = b * (1.0 - p) + tb * p;
        }

        // 3. Film Pipeline
        if s.film != FilmStyle::None && s.f_press > 0.0 {
            let (mut tr, mut tg, mut tb) = (r, g, b);
            match s.film {
                FilmStyle::Kodachrome64 => { tr = r * 1.2 - 0.05; tg = g * 1.1; tb = b * 0.9; }
                FilmStyle::Technicolor2Strip => { tr = r * 1.3 - b * 0.3; tg = g * 0.9; tb = b * 1.2 - r * 0.2; }
                FilmStyle::StandardFilmStock => { tr = r * 1.05 - 0.02; tg = g * 1.05 - 0.02; tb = b * 1.05 - 0.02; }
                FilmStyle::BleachBypass => { let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b; let desat = (r + g + b) / 3.0; tr = r * 0.5 + desat * 0.5 * lum; tg = g * 0.5 + desat * 0.5 * lum; tb = b * 0.5 + desat * 0.5 * lum; }
                FilmStyle::CrossProcess => { tr = r * 1.3 - 0.1; tg = g * 1.0; tb = b * 0.8 + 0.15; }
                FilmStyle::CineTone => { tr = r * 1.12; tg = g * 1.02; tb = b * 0.95; }
                FilmStyle::ArriLook => { tr = r * 1.08; tg = g * 1.05; tb = b * 1.02; }
                FilmStyle::RedEpicStyle => { tr = r * 1.2; tg = g * 0.98; tb = b * 0.92; }
                FilmStyle::Technicolor3Strip => { tr = r * 1.35 - b * 0.2; tg = g * 1.1; tb = b * 1.25 - r * 0.1; }
                FilmStyle::None => {}
            }
            let p = s.f_press; r = r * (1.0 - p) + tr * p; g = g * (1.0 - p) + tg * p; b = b * (1.0 - p) + tb * p;
        }

        // 4. Cinema Pipeline
        if s.cinema != CinemaStyle::None && s.c_press > 0.0 {
            let (mut tr, mut tg, mut tb) = (r, g, b);
            match s.cinema {
                CinemaStyle::TealAndOrange => { let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b; tr = r + (0.2 - lum) * 0.3; tg = g + (0.1 - lum.abs()) * 0.1; tb = b + (lum - 0.2) * 0.3; }
                CinemaStyle::Blockbuster => { tr = r * 1.15; tg = g * 0.95; tb = b * 1.1; }
                CinemaStyle::DarkThriller => { tr = r * 0.85; tg = g * 0.9; tb = b * 1.15; }
                CinemaStyle::IndieDrama => { tr = r * 1.05; tg = g * 0.98; tb = b * 0.9; }
                CinemaStyle::EpicAdventure => { tr = r * 1.2; tg = g * 1.05; tb = b * 0.9; }
                CinemaStyle::HollywoodGold => { tr = r * 1.25; tg = g * 1.15; tb = b * 0.75; }
                CinemaStyle::ActionBlockbuster => { tr = r * 1.3; tg = g * 0.9; tb = b * 1.2; }
                CinemaStyle::SciFiBlue => { tr = r * 0.85; tg = g * 1.05; tb = b * 1.35; }
                CinemaStyle::NeoNoir => { let lum = 0.299 * r + 0.587 * g + 0.114 * b; tr = lum * 1.1; tg = lum * 0.9; tb = lum * 1.2; }
                CinemaStyle::None => {}
            }
            let p = s.c_press; r = r * (1.0 - p) + tr * p; g = g * (1.0 - p) + tg * p; b = b * (1.0 - p) + tb * p;
        }

        // 5. Moody Pipeline
        if s.moody != MoodyStyle::None && s.m_press > 0.0 {
            let (mut tr, mut tg, mut tb) = (r, g, b);
            match s.moody {
                MoodyStyle::MysteryTone => { let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b; tr = lum * 0.9; tg = lum * 0.95; tb = lum * 1.2; }
                MoodyStyle::AlpineChill => { tr = r * 0.9; tg = g * 1.02; tb = b * 1.25; }
                MoodyStyle::DeepForest => { tr = r * 0.85; tg = g * 1.2; tb = b * 0.9; }
                MoodyStyle::AutumnHaze => { tr = r * 1.2; tg = g * 1.05; tb = b * 0.7; }
                MoodyStyle::ArcticFrost => { tr = r * 0.85; tg = g * 1.1; tb = b * 1.35; }
                MoodyStyle::SomberGray => { let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b; tr = lum * 0.95; tg = lum * 0.95; tb = lum * 1.0; }
                MoodyStyle::StormyBlue => { tr = r * 0.8; tg = g * 0.9; tb = b * 1.3; }
                MoodyStyle::TwilightGlow => { tr = r * 1.1; tg = g * 0.85; tb = b * 1.2; }
                MoodyStyle::FoggyHaze => { tr = r * 0.9 + 0.1; tg = g * 0.9 + 0.1; tb = b * 0.95 + 0.05; }
                MoodyStyle::None => {}
            }
            let p = s.m_press; r = r * (1.0 - p) + tr * p; g = g * (1.0 - p) + tg * p; b = b * (1.0 - p) + tb * p;
        }

        // 6. Instant Pipeline
        if s.instant != InstantStyle::None && s.i_press > 0.0 {
            let (mut tr, mut tg, mut tb) = (r, g, b);
            match s.instant {
                InstantStyle::PolaroidExpired => { tr = r * 1.1 + 0.05; tg = g * 0.95 + 0.02; tb = b * 0.85; }
                InstantStyle::InstaxWarm => { tr = r * 1.12; tg = g * 1.05; tb = b * 0.92; }
                InstantStyle::PolaroidVivid => { tr = r * 1.25; tg = g * 1.15; tb = b * 1.1; }
                InstantStyle::ToyCamera => { tr = ((r - 0.5) * 1.4 + 0.5) * 0.95; tg = ((g - 0.5) * 1.4 + 0.5) * 0.95; tb = ((b - 0.5) * 1.4 + 0.5) * 0.95; }
                InstantStyle::LofiSnap => { tr = r * 0.95 + 0.05; tg = g * 0.95 + 0.05; tb = b * 0.95 + 0.05; }
                InstantStyle::InstaxCool => { tr = r * 0.92; tg = g * 1.02; tb = b * 1.15; }
                InstantStyle::PolaroidFade => { tr = r * 0.8 + 0.15; tg = g * 0.8 + 0.15; tb = b * 0.8 + 0.15; }
                InstantStyle::PolaroidVintage => { tr = r * 1.15; tg = g * 0.9; tb = b * 0.75; }
                InstantStyle::InstantClassic => { tr = r * 1.05; tg = g * 1.02; tb = b * 0.98; }
                InstantStyle::None => {}
            }
            let p = s.i_press; r = r * (1.0 - p) + tr * p; g = g * (1.0 - p) + tg * p; b = b * (1.0 - p) + tb * p;
        }

        // 7. Slide Pipeline
        if s.slide != SlideStyle::None && s.s_press > 0.0 {
            let (mut tr, mut tg, mut tb) = (r, g, b);
            match s.slide {
                SlideStyle::EktachromeWarm => { tr = r * 1.18; tg = g * 1.08; tb = b * 0.95; }
                SlideStyle::VelviaVivid => { tr = r * 1.3; tg = g * 1.25; tb = b * 1.2; }
                SlideStyle::AgfaSlide => { tr = r * 1.1; tg = g * 1.15; tb = b * 0.9; }
                SlideStyle::KodachromeSlide => { tr = r * 1.22; tg = g * 1.02; tb = b * 0.88; }
                SlideStyle::HighSatSlide => { let l = 0.2126 * r + 0.7152 * g + 0.0722 * b; tr = l + (r - l) * 1.4; tg = l + (g - l) * 1.4; tb = l + (b - l) * 1.4; }
                SlideStyle::SlidePositive => { tr = r * 1.15; tg = g * 1.12; tb = b * 1.1; }
                SlideStyle::FujiSlide => { tr = r * 1.05; tg = g * 1.2; tb = b * 1.1; }
                SlideStyle::RichSlide => { tr = r * 1.25; tg = g * 1.18; tb = b * 1.05; }
                SlideStyle::WarmSlide => { tr = r * 1.2; tg = g * 1.08; tb = b * 0.85; }
                SlideStyle::None => {}
            }
            let p = s.s_press; r = r * (1.0 - p) + tr * p; g = g * (1.0 - p) + tg * p; b = b * (1.0 - p) + tb * p;
        }

        // 8. Fujifilm Pipeline
        if s.fujifilm != FujifilmStyle::None && s.fj_press > 0.0 {
            let (mut tr, mut tg, mut tb) = (r, g, b);
            match s.fujifilm {
                FujifilmStyle::ClassicChrome => { tr = r * 1.02; tg = g * 1.05; tb = b * 0.98; }
                FujifilmStyle::AstiaSoft => { tr = r * 0.98 + 0.02; tg = g * 0.98 + 0.02; tb = b * 0.98 + 0.02; }
                FujifilmStyle::ProviaStandard => { tr = r * 1.05; tg = g * 1.05; tb = b * 1.05; }
                FujifilmStyle::AcrosMono => { let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b; tr = lum; tg = lum; tb = lum; }
                FujifilmStyle::ClassicNeg => { tr = r * 1.1; tg = g * 0.98; tb = b * 0.9; }
                FujifilmStyle::SuperiaFilm => { tr = r * 1.15; tg = g * 1.1; tb = b * 1.05; }
                FujifilmStyle::EternaCinema => { tr = r * 0.95; tg = g * 0.95; tb = b * 0.95; }
                FujifilmStyle::VelviaFilm => { tr = r * 1.3; tg = g * 1.2; tb = b * 1.1; }
                FujifilmStyle::ProNegHi => { tr = r * 1.08; tg = g * 1.02; tb = b * 0.98; }
                FujifilmStyle::None => {}
            }
            let p = s.fj_press; r = r * (1.0 - p) + tr * p; g = g * (1.0 - p) + tg * p; b = b * (1.0 - p) + tb * p;
        }

        // 9. Negative Pipeline
        if s.negative != NegativeStyle::None && s.n_press > 0.0 {
            let (mut tr, mut tg, mut tb) = (r, g, b);
            match s.negative {
                NegativeStyle::PortraWarm => { tr = r * 1.1; tg = g * 1.05; tb = b * 0.95; }
                NegativeStyle::SuperiaVivid => { tr = r * 1.2; tg = g * 1.1; tb = b * 1.15; }
                NegativeStyle::GoldConsumer => { tr = r * 1.15; tg = g * 1.08; tb = b * 0.85; }
                NegativeStyle::CinestillHalation => { tr = r * 1.25; tg = g * 0.95; tb = b * 0.9; }
                NegativeStyle::UltramaxPop => { tr = r * 1.2; tg = g * 1.2; tb = b * 1.05; }
                NegativeStyle::ColorNeg100 => { tr = r * 1.02; tg = g * 1.02; tb = b * 1.02; }
                NegativeStyle::ColorNeg400 => { tr = r * 1.05; tg = g * 1.04; tb = b * 0.98; }
                NegativeStyle::WarmNegative => { tr = r * 1.18; tg = g * 1.05; tb = b * 0.88; }
                NegativeStyle::SoftNegative => { tr = r * 0.95 + 0.05; tg = g * 0.95 + 0.05; tb = b * 0.95 + 0.05; }
                NegativeStyle::None => {}
            }
            let p = s.n_press; r = r * (1.0 - p) + tr * p; g = g * (1.0 - p) + tg * p; b = b * (1.0 - p) + tb * p;
        }

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
    "id": "org.photocraft.filter.motion_pictures_xl",
    "name": "Motion Pictures XL",
    "version": "1.0.0",
    "kind": "filter",
    "author": "CHJ85",
    "description": "Turn your photo into a masterpiece, with this extremely comprehensive collection of cinematic grade style presets.",
    "params": {
    "vintageStyle": {
    "type": "choice",
    "options": ["None", "Sepia Classic", "Faded Polaroid", "Retro 70s", "Cyber Retro", "Grunge Rock", "Vintage Film", "Golden Era", "Antique Warm", "Retro Fade"],
    "default": "None",
    "label": "Vintage Styles"
    },
    "vintagePressure": { "type": "number", "min": 0, "max": 100, "default": 100, "label": "Vintage Pressure" },
    "bnwStyle": {
    "type": "choice",
    "options": ["None", "Cinematic Noir", "High Contrast Mono", "Silver Gelatin", "Monochrome Gold", "DarkMoody Mono", "Classic Mono", "Noir Contrast", "Soft Silver", "Aged Mono"],
    "default": "None",
    "label": "Black & White Styles"
    },
    "bnwPressure": { "type": "number", "min": 0, "max": 100, "default": 100, "label": "Black & White Pressure" },
    "filmStyle": {
    "type": "choice",
    "options": ["None", "Kodachrome 64", "Technicolor 2-Strip", "Standard Film Stock", "Bleach Bypass", "Cross Process (Xpro)", "CineTone", "Arri Look", "Red Epic Style", "Technicolor 3-Strip"],
                                   "default": "None",
                                   "label": "Film Styles"
                                   },
                                   "filmPressure": { "type": "number", "min": 0, "max": 100, "default": 100, "label": "Film Pressure" },
                                   "cinemaStyle": {
                                   "type": "choice",
                                   "options": ["None", "Teal & Orange", "Blockbuster", "Dark Thriller", "Indie Drama", "Epic Adventure", "Hollywood Gold", "Action Blockbuster", "Sci-Fi Blue", "Neo-Noir"],
                                   "default": "None",
                                   "label": "Cinema Styles"
                                   },
                                   "cinemaPressure": { "type": "number", "min": 0, "max": 100, "default": 100, "label": "Cinema Pressure" },
                                   "moodyStyle": {
                                   "type": "choice",
                                   "options": ["None", "Mystery Tone", "Alpine Chill", "Deep Forest", "Autumn Haze", "Arctic Frost", "Somber Gray", "Stormy Blue", "Twilight Glow", "Foggy Haze"],
                                   "default": "None",
                                   "label": "Moody Styles"
                                   },
                                   "moodyPressure": { "type": "number", "min": 0, "max": 100, "default": 100, "label": "Moody Pressure" },
                                   "instantStyle": {
                                   "type": "choice",
                                   "options": ["None", "Polaroid Expired", "Instax Warm", "Polaroid Vivid", "Toy Camera", "LofiSnap", "Instax Cool", "Polaroid Fade", "Polaroid Vintage", "Instant Classic"],
                                   "default": "None",
                                   "label": "Instant Styles"
                                   },
                                   "instantPressure": { "type": "number", "min": 0, "max": 100, "default": 100, "label": "Instant Pressure" },
                                   "slideStyle": {
                                   "type": "choice",
                                   "options": ["None", "Ektachrome Warm", "Velvia Vivid", "Agfa Slide", "Kodachrome Slide", "High Sat Slide", "Slide Positive", "Fuji Slide", "Rich Slide", "Warm Slide"],
                                   "default": "None",
                                   "label": "Slide Styles"
                                   },
                                   "slidePressure": { "type": "number", "min": 0, "max": 100, "default": 100, "label": "Slide Pressure" },
                                   "fujifilmStyle": {
                                   "type": "choice",
                                   "options": ["None", "Classic Chrome", "Astia Soft", "Provia Standard", "Acros Mono", "Classic Neg", "Superia Film", "Eterna Cinema", "Velvia Film", "Pro Neg Hi"],
                                   "default": "None",
                                   "label": "Fujifilm Styles"
                                   },
                                   "fujifilmPressure": { "type": "number", "min": 0, "max": 100, "default": 100, "label": "Fujifilm Pressure" },
                                   "negativeStyle": {
                                   "type": "choice",
                                   "options": ["None", "Portra Warm", "Superia Vivid", "Gold Consumer", "Cinestill Halation", "Ultramax Pop", "Color Neg 100", "Color Neg 400", "Warm Negative", "Soft Negative"],
                                   "default": "None",
                                   "label": "Negative Styles"
                                   },
                                   "negativePressure": { "type": "number", "min": 0, "max": 100, "default": 100, "label": "Negative Pressure" }
                                   },
                                   "overlap": 0,
                                   "area": "canvas"
                                   }"#
);

pub struct MotionPicturesFilter;

export_filter!(MotionPicturesFilter);

impl Filter for MotionPicturesFilter {
    const MANIFEST: &'static str = MANIFEST;

    fn apply(image: &mut Image<'_>, params: &Params) -> Result<(), Error> {
        let settings = MotionSettings::from_params(params);
        apply_motion_pictures(image, &settings)
    }
}
