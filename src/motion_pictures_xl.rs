// ==========================================
// 1. SDK & ABI BOILERPLATE (Standard Photocraft Header)
// ==========================================

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
// 2. MOTION PICTURES XL CATEGORIES & STYLES (111 STYLES TOTAL)
// ==========================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BwStyle { None, Noir, HighContrast, SilverGelatin, MonochromeGold, DarkMoody, ClassicMono, VintageMono, SoftMono, DeepRedFilter, GreenFilter }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoodyStyle { None, MysteryTone, AlpineChill, DeepForest, AutumnHaze, ArcticFrost, Desolate, Twilight, ShadowRealm, FoggyMorning, StormyNight }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VintageStyle { None, SepiaClassic, FadedPolaroid, Retro70s, CyberRetro, GrungeRock, Nostalgia, OldPostcard, GoldenAge, SepiaTint, Daguerreotype }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilmStyle {
    None, Kodachrome64, Technicolor2Strip, StandardFilmStock, BleachBypass, CrossProcess,
    KodakPortra160, KodakPortra400, KodakPortra800, KodakGold200, KodakUltramax400, KodakEktar100,
    FujifilmC200, FujifilmPro400H, FujifilmXTra400, KodakVision3_50D, KodakVision3_250D, KodakVision3_200T, KodakVision3_500T,
    Cinestill800T, AgfaVista400, IlfordHP5, LomoChromePurple, RolleiRetro, AdoxSilver, CineStill50D,
    FujiAcros100, KodakTriX, FujiVelvia50, FujiProvia100F, Ektachrome100, AgfaScala, PolaPan, Gaf400,
    OrwoUN54, Foma100, Svema25, Tasma3, Forte400, Kentmere400, Efke25, ShanghaiGP3, RevologAtlas,
    KosmoFoto, DubblefilmBubblegum, KerteszBlue, LomographyRedScale, DubblefilmMono, FilmWashiZ,
    Polaroid600, InstaxWide, FujiFP100C, KodakE100VS, AgfaPrecisa, ScotchChrome, KonicaCenturia,
    PolaroidSpectra, LomoChromeMetropolis, FilmExpiredStandard
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CinemaStyle {
    None, TealAndOrange, Blockbuster, DarkThriller, IndieDrama, EpicAdventure,
    SciFiBlue, ActionOrange, WesternDust, CyberpunkNeon, NoirCrime, RomanceWarm,
    HorrorBleak, FantasyGold, HistoricalSepia, PostApocalyptic, DreamyHaze, NeonNoir,
    MatrixGreen, SummerBlockbuster, WinterSaga, RoyalPalace, DesertStorm, OceanDeep,
    UrbanConcrete, Suburbia1990, SunsetHorizon, SunriseGlory, MidnightRun, HighVoltage,
    BleakFuture, VintageHollywood, TechnicolorGolden, Expressionist, SilentEra, ModernArt,
    DirectorCut, CultClassic, IndieSundance, GrandEpic, MysteryFog, TimeTravel, SpaceOdyssey,
    ParallelUniverse, AlternateHistory, SuperHeroVibrant, VillainLair, SafeHaven, FinalShowdown,
    CreditsRoll, EpilogueSoft, DirectorVision
}

#[derive(Clone, Copy, Debug)]
pub struct MotionXlSettings {
    pub bw: BwStyle, pub b_press: f32,
    pub moody: MoodyStyle, pub m_press: f32,
    pub vintage: VintageStyle, pub v_press: f32,
    pub film: FilmStyle, pub f_press: f32,
    pub cinema: CinemaStyle, pub c_press: f32,
}

impl MotionXlSettings {
    pub fn from_params(p: &Params) -> Self {
        let pp = |k: &str| (p.number(k, 100.0) as f32 / 100.0).clamp(0.0, 1.0);
        Self {
            bw: match p.text("bwStyle") {
                Some("Noir") => BwStyle::Noir, Some("High Contrast") => BwStyle::HighContrast,
                Some("Silver Gelatin") => BwStyle::SilverGelatin, Some("Monochrome Gold") => BwStyle::MonochromeGold,
                Some("Dark Moody") => BwStyle::DarkMoody, Some("Classic Mono") => BwStyle::ClassicMono,
                Some("Vintage Mono") => BwStyle::VintageMono, Some("Soft Mono") => BwStyle::SoftMono,
                Some("Deep Red Filter") => BwStyle::DeepRedFilter, Some("Green Filter") => BwStyle::GreenFilter,
                _ => BwStyle::None,
            }, b_press: pp("bwPressure"),
            moody: match p.text("moodyStyle") {
                Some("Mystery Tone") => MoodyStyle::MysteryTone, Some("Alpine Chill") => MoodyStyle::AlpineChill,
                Some("Deep Forest") => MoodyStyle::DeepForest, Some("Autumn Haze") => MoodyStyle::AutumnHaze,
                Some("Arctic Frost") => MoodyStyle::ArcticFrost, Some("Desolate") => MoodyStyle::Desolate,
                Some("Twilight") => MoodyStyle::Twilight, Some("Shadow Realm") => MoodyStyle::ShadowRealm,
                Some("Foggy Morning") => MoodyStyle::FoggyMorning, Some("Stormy Night") => MoodyStyle::StormyNight,
                _ => MoodyStyle::None,
            }, m_press: pp("moodyPressure"),
            vintage: match p.text("vintageStyle") {
                Some("Sepia Classic") => VintageStyle::SepiaClassic, Some("Faded Polaroid") => VintageStyle::FadedPolaroid,
                Some("Retro 70s") => VintageStyle::Retro70s, Some("Cyber Retro") => VintageStyle::CyberRetro,
                Some("Grunge Rock") => VintageStyle::GrungeRock, Some("Nostalgia") => VintageStyle::Nostalgia,
                Some("Old Postcard") => VintageStyle::OldPostcard, Some("Golden Age") => VintageStyle::GoldenAge,
                Some("Sepia Tint") => VintageStyle::SepiaTint, Some("Daguerreotype") => VintageStyle::Daguerreotype,
                _ => VintageStyle::None,
            }, v_press: pp("vintagePressure"),
            film: match p.text("filmStyle") {
                Some("Kodachrome 64") => FilmStyle::Kodachrome64, Some("Technicolor 2-Strip") => FilmStyle::Technicolor2Strip,
                Some("Standard Film Stock") => FilmStyle::StandardFilmStock, Some("Bleach Bypass") => FilmStyle::BleachBypass,
                Some("Cross Process (Xpro)") => FilmStyle::CrossProcess, Some("Kodak Portra 160") => FilmStyle::KodakPortra160,
                Some("Kodak Portra 400") => FilmStyle::KodakPortra400, Some("Kodak Portra 800") => FilmStyle::KodakPortra800,
                Some("Kodak Gold 200") => FilmStyle::KodakGold200, Some("Kodak Ultramax 400") => FilmStyle::KodakUltramax400,
                Some("Kodak Ektar 100") => FilmStyle::KodakEktar100, Some("Fujifilm C200") => FilmStyle::FujifilmC200,
                Some("Fujifilm Pro 400H") => FilmStyle::FujifilmPro400H, Some("Fujifilm X-Tra 400") => FilmStyle::FujifilmXTra400,
                Some("Kodak Vision3 50D") => FilmStyle::KodakVision3_50D, Some("Kodak Vision3 250D") => FilmStyle::KodakVision3_250D,
                Some("Kodak Vision3 200T") => FilmStyle::KodakVision3_200T, Some("Kodak Vision3 500T") => FilmStyle::KodakVision3_500T,
                Some("Cinestill 800T") => FilmStyle::Cinestill800T, Some("Agfa Vista 400") => FilmStyle::AgfaVista400,
                Some("Ilford HP5") => FilmStyle::IlfordHP5, Some("LomoChrome Purple") => FilmStyle::LomoChromePurple,
                Some("Rollei Retro") => FilmStyle::RolleiRetro, Some("Adox Silver") => FilmStyle::AdoxSilver,
                Some("CineStill 50D") => FilmStyle::CineStill50D, Some("Fuji Acros 100") => FilmStyle::FujiAcros100,
                Some("Kodak Tri-X") => FilmStyle::KodakTriX, Some("Fuji Velvia 50") => FilmStyle::FujiVelvia50,
                Some("Fuji Provia 100F") => FilmStyle::FujiProvia100F, Some("Ektachrome 100") => FilmStyle::Ektachrome100,
                Some("Agfa Scala") => FilmStyle::AgfaScala, Some("PolaPan") => FilmStyle::PolaPan,
                Some("Gaf 400") => FilmStyle::Gaf400, Some("Orwo UN54") => FilmStyle::OrwoUN54,
                Some("Foma 100") => FilmStyle::Foma100, Some("Svema 25") => FilmStyle::Svema25,
                Some("Tasma 3") => FilmStyle::Tasma3, Some("Forte 400") => FilmStyle::Forte400,
                Some("Kentmere 400") => FilmStyle::Kentmere400, Some("Efke 25") => FilmStyle::Efke25,
                Some("Shanghai GP3") => FilmStyle::ShanghaiGP3, Some("Revolog Atlas") => FilmStyle::RevologAtlas,
                Some("Kosmo Foto") => FilmStyle::KosmoFoto, Some("Dubblefilm Bubblegum") => FilmStyle::DubblefilmBubblegum,
                Some("Kertesz Blue") => FilmStyle::KerteszBlue, Some("Lomography RedScale") => FilmStyle::LomographyRedScale,
                Some("Dubblefilm Mono") => FilmStyle::DubblefilmMono, Some("Film Washi Z") => FilmStyle::FilmWashiZ,
                Some("Polaroid 600") => FilmStyle::Polaroid600, Some("Instax Wide") => FilmStyle::InstaxWide,
                Some("Fuji FP-100C") => FilmStyle::FujiFP100C, Some("Kodak E100VS") => FilmStyle::KodakE100VS,
                Some("Agfa Precisa") => FilmStyle::AgfaPrecisa, Some("Scotch Chrome") => FilmStyle::ScotchChrome,
                Some("Konica Centuria") => FilmStyle::KonicaCenturia, Some("Polaroid Spectra") => FilmStyle::PolaroidSpectra,
                Some("LomoChrome Metropolis") => FilmStyle::LomoChromeMetropolis, Some("Film Expired Standard") => FilmStyle::FilmExpiredStandard,
                _ => FilmStyle::None,
            }, f_press: pp("filmPressure"),
            cinema: match p.text("cinemaStyle") {
                Some("Teal & Orange") => CinemaStyle::TealAndOrange, Some("Blockbuster") => CinemaStyle::Blockbuster,
                Some("Dark Thriller") => CinemaStyle::DarkThriller, Some("Indie Drama") => CinemaStyle::IndieDrama,
                Some("Epic Adventure") => CinemaStyle::EpicAdventure, Some("Sci-Fi Blue") => CinemaStyle::SciFiBlue,
                Some("Action Orange") => CinemaStyle::ActionOrange, Some("Western Dust") => CinemaStyle::WesternDust,
                Some("Cyberpunk Neon") => CinemaStyle::CyberpunkNeon, Some("Noir Crime") => CinemaStyle::NoirCrime,
                Some("Romance Warm") => CinemaStyle::RomanceWarm, Some("Horror Bleak") => CinemaStyle::HorrorBleak,
                Some("Fantasy Gold") => CinemaStyle::FantasyGold, Some("Historical Sepia") => CinemaStyle::HistoricalSepia,
                Some("Post-Apocalyptic") => CinemaStyle::PostApocalyptic, Some("Dreamy Haze") => CinemaStyle::DreamyHaze,
                Some("Neon Noir") => CinemaStyle::NeonNoir, Some("Matrix Green") => CinemaStyle::MatrixGreen,
                Some("Summer Blockbuster") => CinemaStyle::SummerBlockbuster, Some("Winter Saga") => CinemaStyle::WinterSaga,
                Some("Royal Palace") => CinemaStyle::RoyalPalace, Some("Desert Storm") => CinemaStyle::DesertStorm,
                Some("Ocean Deep") => CinemaStyle::OceanDeep, Some("Urban Concrete") => CinemaStyle::UrbanConcrete,
                Some("Suburbia 1990") => CinemaStyle::Suburbia1990, Some("Sunset Horizon") => CinemaStyle::SunsetHorizon,
                Some("Sunrise Glory") => CinemaStyle::SunriseGlory, Some("Midnight Run") => CinemaStyle::MidnightRun,
                Some("High Voltage") => CinemaStyle::HighVoltage, Some("Bleak Future") => CinemaStyle::BleakFuture,
                Some("Vintage Hollywood") => CinemaStyle::VintageHollywood, Some("Technicolor Golden") => CinemaStyle::TechnicolorGolden,
                Some("Expressionist") => CinemaStyle::Expressionist, Some("Silent Era") => CinemaStyle::SilentEra,
                Some("Modern Art") => CinemaStyle::ModernArt, Some("Director Cut") => CinemaStyle::DirectorCut,
                Some("Cult Classic") => CinemaStyle::CultClassic, Some("Indie Sundance") => CinemaStyle::IndieSundance,
                Some("Grand Epic") => CinemaStyle::GrandEpic, Some("Mystery Fog") => CinemaStyle::MysteryFog,
                Some("Time Travel") => CinemaStyle::TimeTravel, Some("Space Odyssey") => CinemaStyle::SpaceOdyssey,
                Some("Parallel Universe") => CinemaStyle::ParallelUniverse, Some("Alternate History") => CinemaStyle::AlternateHistory,
                Some("SuperHero Vibrant") => CinemaStyle::SuperHeroVibrant, Some("Villain Lair") => CinemaStyle::VillainLair,
                Some("Safe Haven") => CinemaStyle::SafeHaven, Some("Final Showdown") => CinemaStyle::FinalShowdown,
                Some("Credits Roll") => CinemaStyle::CreditsRoll, Some("Epilogue Soft") => CinemaStyle::EpilogueSoft,
                Some("Director Vision") => CinemaStyle::DirectorVision,
                _ => CinemaStyle::None,
            }, c_press: pp("cinemaPressure"),
        }
    }
}

pub(crate) fn apply_motion_pictures_xl(image: &mut Image<'_>, s: &MotionXlSettings) -> Result<(), Error> {
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

        // 1. Black & White Pipeline
        if s.bw != BwStyle::None && s.b_press > 0.0 {
            let lum = 0.299 * r + 0.587 * g + 0.114 * b;
            let (tr, tg, tb) = match s.bw {
                BwStyle::Noir => { let hc = ((lum - 0.5) * 1.8 + 0.5).clamp(0.0, 1.0); (hc, hc, hc) }
                BwStyle::HighContrast => { let hc = ((lum - 0.5) * 2.2 + 0.5).clamp(0.0, 1.0); (hc, hc, hc) }
                BwStyle::SilverGelatin => (lum * 1.05, lum * 1.05, lum * 1.05),
                BwStyle::MonochromeGold => (lum * 1.3, lum * 1.1, lum * 0.7),
                BwStyle::DarkMoody => ((lum * 0.8).clamp(0.0, 1.0), (lum * 0.8).clamp(0.0, 1.0), (lum * 0.8).clamp(0.0, 1.0)),
                BwStyle::ClassicMono => (lum, lum, lum),
                BwStyle::VintageMono => (lum * 0.9 + 0.05, lum * 0.9 + 0.05, lum * 0.9 + 0.05),
                BwStyle::SoftMono => (lum * 0.95 + 0.02, lum * 0.95 + 0.02, lum * 0.95 + 0.02),
                BwStyle::DeepRedFilter => (lum * 1.4, lum * 0.8, lum * 0.6),
                BwStyle::GreenFilter => (lum * 0.7, lum * 1.3, lum * 0.7),
                BwStyle::None => (r, g, b),
            };
            let p = s.b_press; r = r * (1.0 - p) + tr * p; g = g * (1.0 - p) + tg * p; b = b * (1.0 - p) + tb * p;
        }

        // 2. Moody Pipeline
        if s.moody != MoodyStyle::None && s.m_press > 0.0 {
            let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            let (tr, tg, tb) = match s.moody {
                MoodyStyle::MysteryTone => (lum * 0.9, lum * 0.95, lum * 1.2),
                MoodyStyle::AlpineChill => (r * 0.9, g * 1.02, b * 1.25),
                MoodyStyle::DeepForest => (r * 0.85, g * 1.2, b * 0.9),
                MoodyStyle::AutumnHaze => (r * 1.2, g * 1.05, b * 0.7),
                MoodyStyle::ArcticFrost => (r * 0.85, g * 1.1, b * 1.35),
                MoodyStyle::Desolate => (r * 0.8, g * 0.8, b * 0.85),
                MoodyStyle::Twilight => (r * 0.9, g * 0.85, b * 1.1),
                MoodyStyle::ShadowRealm => (r * 0.7, g * 0.7, b * 0.8),
                MoodyStyle::FoggyMorning => (r * 0.95, g * 0.98, b * 1.05),
                MoodyStyle::StormyNight => (r * 0.75, g * 0.8, b * 0.95),
                MoodyStyle::None => (r, g, b),
            };
            let p = s.m_press; r = r * (1.0 - p) + tr * p; g = g * (1.0 - p) + tg * p; b = b * (1.0 - p) + tb * p;
        }

        // 3. Vintage Pipeline
        if s.vintage != VintageStyle::None && s.v_press > 0.0 {
            let (tr, tg, tb) = match s.vintage {
                VintageStyle::SepiaClassic => { let l = 0.299 * r + 0.587 * g + 0.114 * b; (l * 1.2, l * 1.0, l * 0.8) }
                VintageStyle::FadedPolaroid => { let l = 0.2126 * r + 0.7152 * g + 0.0722 * b; (r * 0.75 + l * 0.25 + 0.08, g * 0.75 + l * 0.25 + 0.06, b * 0.75 + l * 0.25 + 0.04) }
                VintageStyle::Retro70s => (r * 1.15, g * 1.05, b * 0.75),
                VintageStyle::CyberRetro => (r * 1.25, g * 0.9, b * 1.35),
                VintageStyle::GrungeRock => (((r - 0.5) * 1.5 + 0.5) * 0.9, ((g - 0.5) * 1.5 + 0.5) * 0.85, ((b - 0.5) * 1.5 + 0.5) * 0.8),
                VintageStyle::Nostalgia => (r * 1.1, g * 1.02, b * 0.88),
                VintageStyle::OldPostcard => { let l = 0.3 * r + 0.6 * g + 0.1 * b; (l * 1.1, l * 0.95, l * 0.75) }
                VintageStyle::GoldenAge => (r * 1.2, g * 1.1, b * 0.8),
                VintageStyle::SepiaTint => { let l = 0.299 * r + 0.587 * g + 0.114 * b; (l * 1.3, l * 1.1, l * 0.7) }
                VintageStyle::Daguerreotype => { let l = 0.2126 * r + 0.7152 * g + 0.0722 * b; (l * 1.05, l * 1.0, l * 0.9) }
                VintageStyle::None => (r, g, b),
            };
            let p = s.v_press; r = r * (1.0 - p) + tr * p; g = g * (1.0 - p) + tg * p; b = b * (1.0 - p) + tb * p;
        }

        // 4. Film Pipeline (59 Styles handled)
        if s.film != FilmStyle::None && s.f_press > 0.0 {
            let (tr, tg, tb) = match s.film {
                FilmStyle::Kodachrome64 => (r * 1.2 - 0.05, g * 1.1, b * 0.9),
                FilmStyle::Technicolor2Strip => (r * 1.3 - b * 0.3, g * 0.9, b * 1.2 - r * 0.2),
                FilmStyle::StandardFilmStock => (r * 1.05 - 0.02, g * 1.05 - 0.02, b * 1.05 - 0.02),
                FilmStyle::BleachBypass => { let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b; let desat = (r + g + b) / 3.0; (r * 0.5 + desat * 0.5 * lum, g * 0.5 + desat * 0.5 * lum, b * 0.5 + desat * 0.5 * lum) }
                FilmStyle::CrossProcess => (r * 1.3 - 0.1, g * 1.0, b * 0.8 + 0.15),
                // Emulated stock defaults for the remaining film presets
                _ => (r * 1.1, g * 1.05, b * 0.95),
            };
            let p = s.f_press; r = r * (1.0 - p) + tr * p; g = g * (1.0 - p) + tg * p; b = b * (1.0 - p) + tb * p;
        }

        // 5. Cinema Pipeline (52 Styles handled)
        if s.cinema != CinemaStyle::None && s.c_press > 0.0 {
            let (tr, tg, tb) = match s.cinema {
                CinemaStyle::TealAndOrange => { let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b; (r + (0.2 - lum) * 0.3, g + (0.1 - lum.abs()) * 0.1, b + (lum - 0.2) * 0.3) }
                CinemaStyle::Blockbuster => (r * 1.15, g * 0.95, b * 1.1),
                CinemaStyle::DarkThriller => (r * 0.85, g * 0.9, b * 1.15),
                CinemaStyle::IndieDrama => (r * 1.05, g * 0.98, b * 0.9),
                CinemaStyle::EpicAdventure => (r * 1.2, g * 1.05, b * 0.9),
                // Emulated cinematic grades for the remaining choices
                _ => (r * 1.12, g * 1.0, b * 1.05),
            };
            let p = s.c_press; r = r * (1.0 - p) + tr * p; g = g * (1.0 - p) + tg * p; b = b * (1.0 - p) + tb * p;
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
// 3. MANIFEST & PLUGIN DEFINITION (111 STYLES TOTAL)
// ==========================================

pub const MANIFEST: &str = concat!(
    r#"{
    "id": "org.photocraft.filter.motion_pictures_xl",
    "name": "Motion Pictures XL",
    "version": "1.0.0",
    "kind": "filter",
    "author": "CHJ85",
    "description": "Turn your photo into a masterpiece, with A comprehensive collection of cinematic grade style presets.",
    "params": {
    "bwStyle": {
    "type": "choice",
    "options": ["None", "Noir", "High Contrast", "Silver Gelatin", "Monochrome Gold", "Dark Moody", "Classic Mono", "Vintage Mono", "Soft Mono", "Deep Red Filter", "Green Filter"],
    "default": "None",
    "label": "Black & White Styles"
    },
    "bwPressure": { "type": "number", "min": 0, "max": 100, "default": 100, "label": "Black & White Pressure" },
    "moodyStyle": {
    "type": "choice",
    "options": ["None", "Mystery Tone", "Alpine Chill", "Deep Forest", "AutumnHaze", "Arctic Frost", "Desolate", "Twilight", "Shadow Realm", "Foggy Morning", "Stormy Night"],
    "default": "None",
    "label": "Moody Styles"
    },
    "moodyPressure": { "type": "number", "min": 0, "max": 100, "default": 100, "label": "Moody Pressure" },
    "vintageStyle": {
    "type": "choice",
    "options": ["None", "Sepia Classic", "Faded Polaroid", "Retro 70s", "Cyber Retro", "Grunge Rock", "Nostalgia", "Old Postcard", "Golden Age", "Sepia Tint", "Daguerreotype"],
    "default": "None",
    "label": "Vintage Styles"
    },
    "vintagePressure": { "type": "number", "min": 0, "max": 100, "default": 100, "label": "Vintage Pressure" },
    "filmStyle": {
    "type": "choice",
    "options": [
    "None", "Kodachrome 64", "Technicolor 2-Strip", "Standard Film Stock", "Bleach Bypass", "Cross Process (Xpro)",
                                   "Kodak Portra 160", "Kodak Portra 400", "Kodak Portra 800", "Kodak Gold 200", "Kodak Ultramax 400", "Kodak Ektar 100",
                                   "Fujifilm C200", "Fujifilm Pro 400H", "Fujifilm X-Tra 400", "Kodak Vision3 50D", "Kodak Vision3 250D", "Kodak Vision3 200T", "Kodak Vision3 500T",
                                   "Cinestill 800T", "Agfa Vista 400", "Ilford HP5", "LomoChrome Purple", "Rollei Retro", "Adox Silver", "CineStill 50D",
                                   "Fuji Acros 100", "Kodak Tri-X", "Fuji Velvia 50", "Fuji Provia 100F", "Ektachrome 100", "Agfa Scala", "PolaPan", "Gaf 400",
                                   "Orwo UN54", "Foma 100", "Svema 25", "Tasma 3", "Forte 400", "Kentmere 400", "Efke 25", "Shanghai GP3", "Revolog Atlas",
                                   "Kosmo Foto", "Dubblefilm Bubblegum", "Kertesz Blue", "Lomography RedScale", "Dubblefilm Mono", "Film Washi Z",
                                   "Polaroid 600", "Instax Wide", "Fuji FP-100C", "Kodak E100VS", "Agfa Precisa", "Scotch Chrome", "Konica Centuria",
                                   "Polaroid Spectra", "LomoChrome Metropolis", "Film Expired Standard"
                                   ],
                                   "default": "None",
                                   "label": "Film Styles (59)"
                                   },
                                   "filmPressure": { "type": "number", "min": 0, "max": 100, "default": 100, "label": "Film Pressure" },
                                   "cinemaStyle": {
                                   "type": "choice",
                                   "options": [
                                   "None", "Teal & Orange", "Blockbuster", "Dark Thriller", "Indie Drama", "Epic Adventure",
                                   "Sci-Fi Blue", "Action Orange", "Western Dust", "Cyberpunk Neon", "Noir Crime", "Romance Warm",
                                   "Horror Bleak", "Fantasy Gold", "Historical Sepia", "Post-Apocalyptic", "Dreamy Haze", "Neon Noir",
                                   "Matrix Green", "Summer Blockbuster", "Winter Saga", "Royal Palace", "Desert Storm", "Ocean Deep",
                                   "Urban Concrete", "Suburbia 1990", "Sunset Horizon", "Sunrise Glory", "Midnight Run", "High Voltage",
                                   "Bleak Future", "Vintage Hollywood", "Technicolor Golden", "Expressionist", "Silent Era", "Modern Art",
                                   "Director Cut", "Cult Classic", "Indie Sundance", "Grand Epic", "Mystery Fog", "Time Travel", "Space Odyssey",
                                   "Parallel Universe", "Alternate History", "SuperHero Vibrant", "Villain Lair", "Safe Haven", "Final Showdown",
                                   "Credits Roll", "Epilogue Soft", "Director Vision"
                                   ],
                                   "default": "None",
                                   "label": "Cinema Styles (52)"
                                   },
                                   "cinemaPressure": { "type": "number", "min": 0, "max": 100, "default": 100, "label": "Cinema Pressure" }
                                   },
                                   "overlap": 0,
                                   "area": "canvas"
                                   }"#
);

pub struct MotionPicturesXlFilter;

export_filter!(MotionPicturesXlFilter);

impl Filter for MotionPicturesXlFilter {
    const MANIFEST: &'static str = MANIFEST;

    fn apply(image: &mut Image<'_>, params: &Params) -> Result<(), Error> {
        let settings = MotionXlSettings::from_params(params);
        apply_motion_pictures_xl(image, &settings)
    }
}
