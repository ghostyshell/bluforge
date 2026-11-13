//! Cinavia detection and removal.
//!
//! Cinavia is Verance's in-band audio watermark. There is no public decoder, so
//! this module does not try to read the payload. It measures the artifact the
//! watermark leaves and removes that artifact.
//!
//! What the watermark actually does (Verance's own account, and DVD-Ranger's
//! 2013 CinEx white paper): it is a narrow-band spread-spectrum signal mixed
//! into 400-4000 Hz, it is *amplitude modulated by the programme*, and the
//! encoder adds slow per-band speed variations as a jammer. Because it scales
//! with the programme, the watermark is invisible in loud passages and becomes
//! the dominant content in the quiet ones.
//!
//! That gives a content-independent measurement: average the spectrum over the
//! quietest seconds of a track, strip the spectral tilt, and count narrow
//! ridges. Programme material rarely leaves ridges in silence; a Cinavia track
//! leaves dozens, concentrated at 300-600 Hz, and the same ridge field appears
//! on every language dub of the same title because the dubs are watermarked
//! from the same master.
//!
//! Removal is a band-stop over that ridge field with raised-cosine edges, a
//! shallower cut across the rest of the band, and a small resample to break the
//! encoder's speed-variation lock. The depth is tuned by re-running the
//! detector on the output until the ridge count falls to the clean floor.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;

use crate::rip::ffmpeg_path;

/// Watermark band as measured on real, independently dubbed tracks.
pub const RIDGE_BAND: (f32, f32) = (300.0, 600.0);
/// Widest band the detections ever reach; used by `--aggressive`.
pub const FULL_BAND: (f32, f32) = (280.0, 4000.0);
/// Attack band of the default strip.
pub const DEFAULT_BAND: (f32, f32) = (280.0, 1300.0);
/// Secondary, shallower cut that covers the rest of the watermark band.
pub const BED: (f32, f32) = (1300.0, 4000.0);

/// Ridge count (in 2.9 Hz bins, 300-600 Hz) at or above which a track is
/// treated as watermarked. Calibrated on a clean control (0-2 ridges) against
/// watermarked dubs of one title (43-70 ridges).
pub const LIKELY: usize = 8;
/// Count at or above which the verdict is "strong".
pub const STRONG: usize = 20;
/// Density a cleaned track is driven down to.
pub const CLEAN: usize = 6;

/// Analysis frame. Finer than the stripper's because the watermark's
/// sidebands are only a few Hz apart and merge at coarser resolution.
const FFT: usize = 16_384;
const FFT_STRIDE: usize = 4096;
/// Half-width of the running median that removes the spectral tilt.
const DETREND_HZ: f32 = 470.0;
/// Ridge gap that closes a detected span, in Hz.
const SPAN_GAP_HZ: f32 = 300.0;

const STFT: usize = 4096;
/// Envelope rate for the modulation measurement, in frames per second.
const ENV_RATE: f32 = 100.0;
const ENV_FRAME: usize = 512;
const HOP: usize = 1024;
const RATE: u32 = 48_000;
const MAX_ANALYZE_SECS: u32 = 600;
const QUIET_BLOCKS: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Clean,
    Likely,
    Strong,
}

/// Ridge mass at or above which a track is treated as watermarked. A clean
/// control measured 1.2; watermarked dubs of one title measured 58-150.
pub const MASS: f32 = 15.0;
/// Mass at or above which the verdict is "strong".
pub const MASS_STRONG: f32 = 60.0;
/// Modulation peak-to-floor ratio of the in-band envelope at or above which a
/// track reads as strongly watermarked. A commentary track measured 9, three
/// watermarked dubs measured 46-53, and a notched copy fell to 28.
pub const MOD_PEAK: f32 = 20.0;
/// In-band level over the high band at or above which a track is watermarked.
/// A clean control measured 21; watermarked dubs measured 36-41.
pub const RATIO_CLEAN: f32 = 27.0;

impl Verdict {
    fn of(density: usize, mass: f32, ratio_db: f32, modpeak: f32) -> Self {
        if density < LIKELY || mass < MASS || ratio_db < RATIO_CLEAN {
            Verdict::Clean
        } else if density >= STRONG && mass >= MASS_STRONG && modpeak >= MOD_PEAK {
            Verdict::Strong
        } else {
            Verdict::Likely
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Verdict::Clean => "clean",
            Verdict::Likely => "watermark likely",
            Verdict::Strong => "watermark present",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TrackReport {
    pub track: usize,
    /// In-band vs high-band energy of the quiet frames, in dB. This is the
    /// texture-independent half of the measurement: a notch cannot fake it.
    pub band_ratio_db: f32,
    /// Channel the measurement was taken from (centre channel for surround).
    pub channel: usize,
    pub channels: usize,
    pub seconds: u32,
    pub density: usize,
    pub wide_density: usize,
    pub mean_excess_db: f32,
    /// Sum of the ridge heights above the detection floor, in dB. Tilt-free.
    pub score: f32,
    /// Peak-to-median ratio of the 300-600 Hz envelope modulation spectrum.
    /// A watermark is modulated, so it leaves a peak the programme does not.
    pub modpeak: f32,
    pub verdict: Verdict,
    /// Band to cut, when the track is watermarked.
    pub band: Option<(f32, f32)>,
    /// Recommended band-stop depth in dB.
    pub depth_db: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct BandSpec {
    pub lo: f32,
    pub hi: f32,
    pub depth_db: f32,
    pub bed_db: f32,
}

impl BandSpec {
    pub fn default_for(band: (f32, f32)) -> Self {
        Self {
            lo: band.0,
            hi: band.1,
            depth_db: 10.0,
            bed_db: 4.0,
        }
    }

    /// The full 280-4000 Hz band with a deeper cut, for stubborn tracks.
    pub fn aggressive() -> Self {
        Self {
            lo: FULL_BAND.0,
            hi: FULL_BAND.1,
            depth_db: 12.0,
            bed_db: 3.0,
        }
    }
}

fn ffprobe_path() -> std::path::PathBuf {
    let ff = ffmpeg_path();
    let name = if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    };
    ff.with_file_name(name)
}

/// What an audio stream is, as far as the encoder choice needs to know.
#[derive(Debug, Clone)]
pub struct SourceAudio {
    pub channels: usize,
    pub codec: String,
    pub profile: Option<String>,
    pub kbps: Option<u32>,
    /// Dolby Atmos (JOC) metadata on a TrueHD or E-AC-3 track.
    pub atmos: bool,
}

/// Describe one audio stream.
pub fn probe(path: &Path, track: usize) -> Result<SourceAudio, String> {
    let out = Command::new(ffprobe_path())
        .args(["-v", "error", "-select_streams", &format!("a:{track}")])
        .args([
            "-show_entries",
            "stream=codec_name,profile,channels,bit_rate,sample_rate:stream_tags=title,handler_name",
            "-of",
            "default=noprint_wrappers=1",
        ])
        .arg(path)
        .output()
        .map_err(|e| format!("ffprobe: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut info = SourceAudio {
        channels: 2,
        codec: String::new(),
        profile: None,
        kbps: None,
        atmos: false,
    };
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() || value == "N/A" || value == "unknown" {
            continue;
        }
        match key.trim() {
            "channels" => {
                if let Ok(n) = value.parse::<usize>() {
                    info.channels = n.max(1);
                }
            }
            "bit_rate" => info.kbps = value.parse::<u32>().ok().map(|b| b / 1000),
            "codec_name" => info.codec = value.to_string(),
            "TAG:title" | "TAG:handler_name" => {
                if value.to_ascii_lowercase().contains("atmos") {
                    info.atmos = true;
                }
            }
            "profile" => {
                if value.to_ascii_lowercase().contains("atmos") {
                    info.atmos = true;
                }
                info.profile = Some(value.to_string());
            }
            _ => {}
        }
    }
    if info.codec.is_empty() {
        info.codec = "pcm".into();
    }
    Ok(info)
}

/// Number of audio streams.
pub fn track_count(path: &Path) -> Result<usize, String> {
    let out = Command::new(ffprobe_path())
        .args([
            "-v",
            "error",
            "-select_streams",
            "a",
            "-show_entries",
            "stream=index",
        ])
        .args(["-of", "csv=p=0"])
        .arg(path)
        .output()
        .map_err(|e| format!("ffprobe: {e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count())
}

fn window(size: usize) -> Vec<f32> {
    (0..size)
        .map(|i| {
            let x = i as f32 / size as f32;
            // Periodic Hann; the analysis and synthesis windows are identical.
            0.5 - 0.5 * (2.0 * std::f32::consts::PI * x).cos()
        })
        .collect()
}

fn bin_hz() -> f32 {
    RATE as f32 / FFT as f32
}

/// Read raw f32le samples out of a child's stdout.
struct Samples<R: Read> {
    inner: R,
    bytes: Vec<u8>,
    pos: usize,
    filled: usize,
    tail: Vec<f32>,
}

impl<R: Read> Samples<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            bytes: vec![0; HOP * 4],
            pos: 0,
            filled: 0,
            tail: Vec::new(),
        }
    }

    /// Next block of `n` samples, or fewer at the end.
    fn take(&mut self, n: usize) -> std::io::Result<Vec<f32>> {
        let mut out = Vec::with_capacity(n);
        while out.len() < n {
            if self.pos >= self.filled {
                self.filled = self.inner.read(&mut self.bytes)?;
                self.pos = 0;
                if self.filled == 0 {
                    break;
                }
            }
            while self.pos + 4 <= self.filled && out.len() < n {
                let s = f32::from_le_bytes([
                    self.bytes[self.pos],
                    self.bytes[self.pos + 1],
                    self.bytes[self.pos + 2],
                    self.bytes[self.pos + 3],
                ]);
                self.pos += 4;
                out.push(s);
            }
        }
        self.tail.extend_from_slice(&out);
        Ok(out)
    }
}

/// Decode one track. `mono` extracts a single channel with `pan`, which keeps
/// the analysis buffer one sample per frame position; without it every channel
/// is kept for the stripper.
fn decode_cmd(
    path: &Path,
    track: usize,
    channels: usize,
    secs: Option<u32>,
    mono: Option<usize>,
) -> Command {
    let mut cmd = Command::new(ffmpeg_path());
    cmd.args(["-v", "error", "-nostdin"]);
    if let Some(s) = secs {
        cmd.args(["-t", &s.to_string()]);
    }
    cmd.arg("-i").arg(path);
    cmd.args(["-map", &format!("0:a:{track}"), "-vn", "-sn", "-dn"]);
    match mono {
        Some(c) => cmd.args(["-af", &format!("pan=mono|c0=c{c}"), "-ac", "1"]),
        None => cmd.args(["-ac", &channels.to_string()]),
    };
    cmd.args(["-ar", &RATE.to_string()]);
    cmd.args(["-acodec", "pcm_f32le", "-f", "f32le", "pipe:1"]);
    cmd.stdout(Stdio::piped()).stderr(Stdio::null());
    cmd
}

/// Measure one track. `secs` caps how much is decoded (default 10 minutes from
/// the start; the watermark is continuous so a prefix is representative).
pub fn analyze(
    path: &Path,
    track: usize,
    channels: usize,
    secs: Option<u32>,
) -> Result<TrackReport, String> {
    let secs = secs.unwrap_or(MAX_ANALYZE_SECS).min(MAX_ANALYZE_SECS);
    let channel = if channels >= 3 { 2 } else { 0 };
    let mut child = decode_cmd(path, track, channels, Some(secs), Some(channel))
        .spawn()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    let mut samples = Samples::new(child.stdout.take().ok_or("no ffmpeg stdout")?);

    let win = window(FFT);
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT);
    let bins = FFT / 2 + 1;
    let mut spectra: Vec<Vec<f32>> = Vec::new();
    let mut levels: Vec<f32> = Vec::new();
    let mut frame = vec![Complex32::default(); FFT];

    let env_cmd = window(ENV_FRAME);
    let env_fft = planner.plan_fft_forward(ENV_FRAME);
    let env_lo = (RIDGE_BAND.0 / RATE as f32 * ENV_FRAME as f32).round() as usize;
    let env_hi = (RIDGE_BAND.1 / RATE as f32 * ENV_FRAME as f32).round() as usize;
    let mut envelope: Vec<f32> = Vec::new();
    let mut env_buf = vec![Complex32::default(); ENV_FRAME];

    // One averaged PSD per second of audio.
    loop {
        let block = samples.take(RATE as usize).map_err(|e| e.to_string())?;
        if block.is_empty() {
            break;
        }
        // In-band envelope at ENV_RATE, for the modulation measurement.
        for chunk in block.chunks((RATE as f32 / ENV_RATE) as usize) {
            if chunk.len() < ENV_FRAME / 2 {
                break;
            }
            for i in 0..ENV_FRAME {
                env_buf[i] = Complex32::new(chunk.get(i).copied().unwrap_or(0.0) * env_cmd[i], 0.0);
            }
            env_fft.process(&mut env_buf);
            let sum: f32 = env_buf[env_lo..env_hi.min(env_buf.len())]
                .iter()
                .map(|c| c.norm())
                .sum();
            envelope.push(sum);
        }

        let mut psd = vec![0.0f32; bins];
        let mut frames = 0usize;
        let mut offset = 0usize;
        while offset + FFT <= block.len() {
            for i in 0..FFT {
                frame[i] = Complex32::new(block[offset + i] * win[i], 0.0);
            }
            fft.process(&mut frame);
            for (b, v) in frame.iter().take(bins).enumerate() {
                psd[b] += v.norm_sqr();
            }
            frames += 1;
            offset += FFT_STRIDE;
        }
        // Trailing part of the last block: zero-pad one frame.
        if frames == 0 && block.len() > FFT / 4 {
            for i in 0..FFT {
                frame[i] = Complex32::new(block.get(i).copied().unwrap_or(0.0) * win[i], 0.0);
            }
            fft.process(&mut frame);
            for (b, v) in frame.iter().take(bins).enumerate() {
                psd[b] += v.norm_sqr();
            }
            frames = 1;
        }
        if frames == 0 {
            continue;
        }
        for v in &mut psd {
            *v /= frames as f32;
        }
        let level: f32 = block.iter().map(|s| s * s).sum::<f32>() / block.len() as f32;
        levels.push(10.0 * (level + 1e-20).log10());
        spectra.push(psd);
    }
    let _ = child.wait();

    // Pick the quietest blocks, skipping the very quietest (digital silence).
    let mut order: Vec<usize> = (0..levels.len()).collect();
    order.sort_by(|a, b| {
        levels[*a]
            .partial_cmp(&levels[*b])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let skip = (order.len() / 50).max(0);
    let picked: Vec<usize> = order.into_iter().skip(skip).take(QUIET_BLOCKS).collect();
    if picked.is_empty() {
        return Err("nothing decoded".into());
    }

    let mut avg = vec![0.0f32; bins];
    for i in &picked {
        for (a, p) in avg.iter_mut().zip(&spectra[*i]) {
            *a += p;
        }
    }
    for a in &mut avg {
        *a /= picked.len() as f32;
    }
    let hz = |i: usize| i as f32 * bin_hz();
    let band_ratio_db = ratio_db(&avg, &hz);
    for a in &mut avg {
        *a = 10.0 * (*a + 1e-20).log10();
    }

    let excess = detrend(&avg);
    let density = count_ridges(&excess, hz, RIDGE_BAND);
    let wide_density = count_ridges(&excess, hz, FULL_BAND);
    let mean_excess_db = {
        let (lo, hi) = RIDGE_BAND;
        let mut sum = 0.0;
        let mut n = 0;
        for (i, e) in excess.iter().enumerate() {
            let f = hz(i);
            if f >= lo && f <= hi {
                sum += *e;
                n += 1;
            }
        }
        if n == 0 {
            0.0
        } else {
            sum / n as f32
        }
    };

    let score = ridge_mass(&excess, &hz);
    let modpeak = modulation_peak(&envelope);
    let verdict = Verdict::of(density, score, band_ratio_db, modpeak);
    let band = (verdict != Verdict::Clean).then(|| {
        let (start, end) = ridge_span(&excess, hz);
        // Widen: the span tracks the strongest ridges, the cut has to cover
        // the sidebands above them as well.
        (
            (start * 0.85).max(DEFAULT_BAND.0),
            (end * 1.6).max(1600.0).min(FULL_BAND.1),
        )
    });
    Ok(TrackReport {
        track,
        band_ratio_db,
        channel,
        channels,
        seconds: levels.len() as u32,
        density,
        wide_density,
        mean_excess_db,
        score,
        modpeak,
        verdict,
        band,
        // Start deeper when the ridge field is far above the clean ratio, so
        // most tracks settle in one pass.
        depth_db: if score >= MASS_STRONG {
            12.0
        } else if score >= MASS * 2.0 {
            10.0
        } else {
            8.0
        },
    })
}

/// Peak-to-median ratio of the in-band envelope's modulation spectrum.
///
/// A spread-spectrum watermark is *modulated* onto the programme, and a
/// modulated carrier leaves a peak in its envelope that plain programme
/// material does not. The envelope is measured at [`ENV_RATE`] from short
/// frames so modulation up to a few Hz survives.
fn modulation_peak(env: &[f32]) -> f32 {
    if env.len() < ENV_RATE as usize {
        return 0.0;
    }
    let mean = env.iter().sum::<f32>() / env.len() as f32;
    if mean <= 1e-20 {
        return 0.0;
    }
    let mut norm: Vec<f32> = env.iter().map(|v| (*v / mean).max(1e-12).ln()).collect();
    let dc = norm.iter().sum::<f32>() / norm.len() as f32;
    for v in &mut norm {
        *v -= dc;
    }
    let size = norm.len().next_power_of_two().max(4096);
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(size);
    let mut buf: Vec<Complex32> = (0..size)
        .map(|i| {
            let w = if i < norm.len() {
                0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / norm.len() as f32).cos()
            } else {
                0.0
            };
            Complex32::new(norm.get(i).copied().unwrap_or(0.0) * w, 0.0)
        })
        .collect();
    fft.process(&mut buf);
    let hz_per_bin = ENV_RATE / size as f32;
    let from = ((0.15 / hz_per_bin).ceil() as usize).max(1);
    let to = ((4.0 / hz_per_bin).floor() as usize)
        .min(size / 2)
        .max(from + 1);
    let power: Vec<f32> = buf[from..to].iter().map(|c| c.norm_sqr()).collect();
    if power.len() < 8 {
        return 0.0;
    }
    let mut sorted = power.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let floor = sorted[sorted.len() / 2].max(1e-20);
    let peak = power.iter().cloned().fold(0.0f32, f32::max);
    peak / floor
}

/// Ridge heights above the detection floor, summed over the ridge band. This
/// is the tilt-free measure the verdict uses: a clean control scores ~1, a
/// watermarked track tens to hundreds.
fn ridge_mass(excess: &[f32], hz: &impl Fn(usize) -> f32) -> f32 {
    excess
        .iter()
        .enumerate()
        .filter(|(i, e)| {
            let f = hz(*i);
            f >= RIDGE_BAND.0 && f <= RIDGE_BAND.1 && **e > 4.0
        })
        .map(|(_, e)| e - 4.0)
        .sum()
}

/// Energy in 300-600 Hz against 8-14 kHz, on an averaged spectrum.
fn ratio_db(avg: &[f32], hz: &impl Fn(usize) -> f32) -> f32 {
    let mean = |lo: f32, hi: f32| {
        let mut sum = 0.0f32;
        let mut n = 0usize;
        for (i, p) in avg.iter().enumerate() {
            let f = hz(i);
            if f >= lo && f <= hi {
                sum += p;
                n += 1;
            }
        }
        if n == 0 {
            -120.0
        } else {
            sum / n as f32
        }
    };
    10.0 * (mean(RIDGE_BAND.0, RIDGE_BAND.1) / mean(8000.0, 14_000.0)).log10()
}

/// Subtract a running median (the spectral tilt), leaving narrow ridges.
fn detrend(db: &[f32]) -> Vec<f32> {
    let half = (DETREND_HZ / bin_hz()).round() as usize;
    let mut out = Vec::with_capacity(db.len());
    let mut scratch: Vec<f32> = Vec::with_capacity(half * 2 + 1);
    for i in 0..db.len() {
        scratch.clear();
        let lo = i.saturating_sub(half);
        let hi = (i + half).min(db.len() - 1);
        scratch.extend_from_slice(&db[lo..=hi]);
        scratch.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        out.push(db[i] - scratch[scratch.len() / 2]);
    }
    out
}

fn count_ridges(excess: &[f32], hz: impl Fn(usize) -> f32, band: (f32, f32)) -> usize {
    excess
        .iter()
        .enumerate()
        .filter(|(i, e)| {
            let f = hz(*i);
            f >= band.0 && f <= band.1 && **e > 4.0
        })
        .count()
}

/// Contiguous span of the ridge field, widened a little for a clean edge.
fn ridge_span(excess: &[f32], hz: impl Fn(usize) -> f32) -> (f32, f32) {
    let mut first: Option<f32> = None;
    let mut last = 0.0f32;
    let mut gap = 0usize;
    let gap_limit = (SPAN_GAP_HZ / bin_hz()).round() as usize;
    for (i, e) in excess.iter().enumerate() {
        let f = hz(i);
        if f < 300.0 || f > FULL_BAND.1 {
            continue;
        }
        if *e > 3.5 {
            gap = 0;
            if first.is_none() {
                first = Some(f);
            }
            last = f;
        } else {
            gap += 1;
            if gap > gap_limit {
                break;
            }
        }
    }
    match first {
        Some(a) => ((a - 40.0).max(200.0), last + 40.0),
        None => DEFAULT_BAND,
    }
}

fn gain_curve(spec: BandSpec) -> Vec<f32> {
    let bins = STFT / 2 + 1;
    let hz = |i: usize| i as f32 * RATE as f32 / STFT as f32;
    let edge = |lo: f32, hi: f32, ramp: f32, f: f32| {
        let up = ((f - (lo - ramp)) / ramp).clamp(0.0, 1.0);
        let down = (((hi + ramp) - f) / ramp).clamp(0.0, 1.0);
        up.min(down)
    };
    let deep = 10f32.powf(-spec.depth_db / 20.0);
    let bed = 10f32.powf(-spec.bed_db / 20.0);
    let mut g = vec![1.0f32; bins];
    for (i, v) in g.iter_mut().enumerate() {
        let f = hz(i);
        let m = edge(spec.lo, spec.hi, 150.0, f);
        let b = edge(BED.0, BED.1, 250.0, f);
        *v = (1.0 - (1.0 - deep) * m) * (1.0 - (1.0 - bed) * b);
    }
    // The inverse transform is fed only the positive frequencies, so the
    // negative mirror has to be folded back in or the output loses 6 dB.
    for (i, v) in g.iter_mut().enumerate() {
        if i != 0 && i != bins - 1 {
            *v *= 2.0;
        }
    }
    g
}

fn write_wav_header(w: &mut impl Write, channels: usize, samples: u32) -> std::io::Result<()> {
    let data = samples * 4 * channels as u32;
    let mut h = Vec::with_capacity(44);
    h.extend_from_slice(b"RIFF");
    h.extend_from_slice(&(36 + data).to_le_bytes());
    h.extend_from_slice(b"WAVEfmt ");
    h.extend_from_slice(&16u32.to_le_bytes());
    h.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
    h.extend_from_slice(&(channels as u16).to_le_bytes());
    h.extend_from_slice(&RATE.to_le_bytes());
    h.extend_from_slice(&(RATE * 4 * channels as u32).to_le_bytes());
    h.extend_from_slice(&((4 * channels) as u16).to_le_bytes());
    h.extend_from_slice(&32u16.to_le_bytes());
    h.extend_from_slice(b"data");
    h.extend_from_slice(&data.to_le_bytes());
    w.write_all(&h)
}

/// Extend the window-sum divisor once per frame (not once per channel).
fn add_norm(norm: &mut Vec<f32>, win: &[f32], base: usize) {
    for (i, w) in win.iter().enumerate().skip(base) {
        let _ = i;
        norm.push(w * w);
    }
    for (n, w) in norm.iter_mut().zip(win).take(base) {
        *n += w * w;
    }
}

/// Commit `take` overlapped samples to the output and slide the buffers.
#[allow(clippy::too_many_arguments)]
fn emit(
    take: usize,
    channels: usize,
    acc: &mut [Vec<f32>],
    norm: &mut Vec<f32>,
    interleaved: &mut Vec<f32>,
    data: &mut u32,
    w: &mut std::io::BufWriter<std::fs::File>,
) -> Result<(), String> {
    interleaved.clear();
    interleaved.resize(take * channels, 0.0);
    for i in 0..take {
        let n = norm[i].max(1e-6);
        for c in 0..channels {
            interleaved[i * channels + c] = acc[c][i] / n;
        }
    }
    for v in interleaved.iter() {
        w.write_all(&v.to_le_bytes()).map_err(|e| e.to_string())?;
    }
    *data += take as u32;
    for lane in acc.iter_mut() {
        lane.drain(..take);
    }
    norm.drain(..take);
    Ok(())
}

/// Target codec for the re-encode. `Auto` follows the source: a track that was
/// Dolby or DTS goes back to the same family, so the result matches the disc
/// rather than gaining a foreign codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Codec {
    Auto,
    Ac3,
    Eac3,
    Dts,
    #[serde(rename = "truehd", alias = "true_hd")]
    TrueHd,
    Flac,
    Aac,
}

impl Codec {
    pub fn parse(name: &str) -> Result<Self, String> {
        Ok(match name.to_ascii_lowercase().as_str() {
            "auto" | "same" => Codec::Auto,
            "ac3" | "dolby" | "dd" => Codec::Ac3,
            "eac3" | "ddp" | "dolby-digital-plus" => Codec::Eac3,
            "dts" => Codec::Dts,
            "truehd" | "true_hd" => Codec::TrueHd,
            "flac" => Codec::Flac,
            "aac" => Codec::Aac,
            other => return Err(format!("unknown codec {other}")),
        })
    }

    /// Map an ffprobe codec name onto the encoder family.
    pub fn from_name(name: &str) -> Self {
        match name.to_ascii_lowercase().as_str() {
            "ac3" => Codec::Ac3,
            "eac3" => Codec::Eac3,
            "dts" => Codec::Dts,
            "truehd" | "mlp" => Codec::TrueHd,
            "flac" => Codec::Flac,
            "aac" => Codec::Aac,
            _ => Codec::Flac,
        }
    }

    /// Map a Blu-ray audio coding type onto the encoder family, plus whether it
    /// is the HD extension of a lossy core.
    pub fn from_bd(coding_type: u8) -> (Self, bool) {
        match coding_type {
            0x81 => (Codec::Ac3, false),
            0x84 | 0xa1 => (Codec::Eac3, false),
            0x82 => (Codec::Dts, false),
            0x85 | 0x86 => (Codec::Dts, true),
            0x83 => (Codec::TrueHd, false),
            0xa2 => (Codec::Dts, false),
            _ => (Codec::Flac, false),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Codec::Auto => "auto",
            Codec::Ac3 => "ac3",
            Codec::Eac3 => "eac3",
            Codec::Dts => "dts",
            Codec::TrueHd => "truehd",
            Codec::Flac => "flac",
            Codec::Aac => "aac",
        }
    }
}

/// A resolved encoder plus the arguments it needs.
#[derive(Debug, Clone)]
pub struct Target {
    pub codec: Codec,
    pub encoder: &'static str,
    pub bitrate: Option<u32>,
    pub strict: bool,
    /// Channels to encode; anything above 5.1 is folded down.
    pub channels: usize,
    pub note: Option<String>,
}

impl Target {
    pub fn label(&self) -> &'static str {
        self.codec.label()
    }
}

/// Largest channel count any of the bundled encoders accepts.
pub const MAX_ENCODER_CHANNELS: usize = 6;

fn clamp_kbps(source_kbps: Option<u32>, lo: u32, hi: u32, fallback: u32) -> Option<u32> {
    let raw = source_kbps.unwrap_or(fallback).clamp(lo, hi);
    // AC-3, E-AC-3 and DTS all encode in 32 kbps steps.
    Some(((raw + 16) / 32) * 32)
}

/// Pick an encoder. `source_codec` is the ffprobe codec name ("ac3", "dts",
/// "truehd", ...) and `profile` its profile string ("DTS-HD MA", ...).
#[allow(clippy::too_many_arguments)]
pub fn target_for(
    want: Codec,
    source: Codec,
    hd: bool,
    atmos: bool,
    source_kbps: Option<u32>,
    channels: usize,
    mkv: bool,
) -> Result<Target, String> {
    let mut note = None;
    let codec = if want == Codec::Auto && atmos && mkv {
        // No Atmos encoder exists, so the objects are gone whatever happens.
        // Keep the bed lossless and at full width instead of folding it to 5.1.
        note = Some("Atmos objects cannot be encoded; keeping the lossless bed as FLAC".into());
        Codec::Flac
    } else if want == Codec::Auto {
        match source {
            // DTS-HD MA has no encoder; its core is lossy, so keep it lossless.
            Codec::Dts if hd && mkv => {
                note = Some("DTS-HD MA cannot be encoded; keeping lossless FLAC".into());
                Codec::Flac
            }
            other => other,
        }
    } else {
        if atmos {
            note = Some("Atmos objects are dropped; only the bed is encoded".into());
        }
        want
    };
    let codec = if !mkv {
        match codec {
            // MP4 has no standard DTS/TrueHD/FLAC mapping here; AAC is the fallback.
            Codec::Dts | Codec::TrueHd | Codec::Flac => {
                if want != Codec::Auto {
                    note = Some(format!(
                        "{} is not carried in MP4; using AAC",
                        codec.label()
                    ));
                }
                Codec::Aac
            }
            other => other,
        }
    } else {
        codec
    };

    let channels = channels.max(1);
    let (encoder, bitrate, strict) = match codec {
        Codec::Ac3 => ("ac3", clamp_kbps(source_kbps, 192, 640, 640), false),
        Codec::Eac3 => ("eac3", clamp_kbps(source_kbps, 192, 1024, 768), false),
        Codec::Dts => ("dca", clamp_kbps(source_kbps, 768, 1536, 1536), true),
        Codec::TrueHd => ("truehd", None, true),
        Codec::Flac => ("flac", None, false),
        // `aac` covers the fallback and the explicit request.
        Codec::Aac | Codec::Auto => ("aac", Some(640), false),
    };
    if codec == Codec::Dts && !mkv {
        return Err("DTS needs an MKV output".into());
    }
    Ok(Target {
        codec,
        encoder,
        bitrate,
        strict,
        channels,
        note,
    })
}

/// Encoder arguments for one output audio stream.
pub fn encode_args(target: &Target) -> Vec<String> {
    let mut a = vec!["-c:a".to_string(), target.encoder.to_string()];
    if let Some(b) = target.bitrate {
        a.push("-b:a".into());
        a.push(format!("{b}k"));
    }
    if target.strict {
        // dca and truehd are still marked experimental in libavcodec.
        a.push("-strict".into());
        a.push("-2".into());
    }
    // Only the Dolby/DTS encoders are capped at 5.1; FLAC and AAC keep 7.1.
    if target.channels > MAX_ENCODER_CHANNELS
        && matches!(
            target.codec,
            Codec::Ac3 | Codec::Eac3 | Codec::Dts | Codec::TrueHd
        )
    {
        a.push("-ac".into());
        a.push(MAX_ENCODER_CHANNELS.to_string());
    }
    a
}

/// Decode `track`, band-stop the watermark, write float32 WAV to `out`.
pub fn strip(
    input: &Path,
    track: usize,
    out: &Path,
    spec: BandSpec,
    channels: usize,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u32),
) -> Result<u64, String> {
    let mut child = decode_cmd(input, track, channels, None, None)
        .spawn()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    let mut samples = Samples::new(child.stdout.take().ok_or("no ffmpeg stdout")?);

    let win = window(STFT);
    let mut planner = FftPlanner::<f32>::new();
    let forward = planner.plan_fft_forward(STFT);
    let inverse = planner.plan_fft_inverse(STFT);
    let gain = gain_curve(spec);
    let bins = STFT / 2 + 1;

    let file = std::fs::File::create(out).map_err(|e| format!("{out:?}: {e}"))?;
    let mut w = std::io::BufWriter::new(file);
    // The header is patched at the end; write a placeholder of the same size.
    write_wav_header(&mut w, channels, 0).map_err(|e| e.to_string())?;

    let mut head: Vec<Vec<f32>> = vec![Vec::new(); channels];
    // Overlap-add buffers: each holds the not-yet-committed tail of recent
    // frames (never more than STFT-HOP samples) and its window-sum divisor.
    let mut acc: Vec<Vec<f32>> = vec![Vec::new(); channels];
    let mut norm: Vec<f32> = Vec::new();
    let mut frame: Vec<Complex32> = vec![Complex32::default(); STFT];
    let mut data_samples = 0u32;
    let mut interleaved: Vec<f32> = Vec::new();
    let mut done = 0usize;

    loop {
        let block = samples.take(HOP * channels).map_err(|e| e.to_string())?;
        if block.is_empty() {
            break;
        }
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("cancelled".into());
        }
        for c in 0..channels {
            let lane = &mut head[c];
            for ch in block.chunks(channels) {
                lane.push(ch.get(c).copied().unwrap_or(0.0));
            }
        }
        while head[0].len() >= STFT {
            let base = acc[0].len();
            add_norm(&mut norm, &win, base);
            for c in 0..channels {
                for i in 0..STFT {
                    frame[i] = Complex32::new(head[c][i] * win[i], 0.0);
                }
                forward.process(&mut frame);
                for (b, v) in frame.iter_mut().take(bins).enumerate() {
                    *v *= gain[b];
                }
                for v in frame.iter_mut().skip(bins) {
                    *v = Complex32::default();
                }
                inverse.process(&mut frame);
                let scale = 1.0 / STFT as f32;
                let lane = &mut acc[c];
                for i in 0..STFT {
                    let y = frame[i].re * scale * win[i];
                    if i < lane.len() {
                        lane[i] += y;
                    } else {
                        lane.push(y);
                    }
                }
                head[c].drain(..HOP);
            }
            let take = HOP.min(norm.len());
            emit(
                take,
                channels,
                &mut acc,
                &mut norm,
                &mut interleaved,
                &mut data_samples,
                &mut w,
            )?;
            done += take;
            if done % 48_000 < HOP {
                progress((data_samples as f32 / (RATE as f32 * channels as f32)) as u32);
            }
        }
    }

    // Flush: feed enough silence to push the overlap-add tail through.
    for lane in head.iter_mut() {
        lane.resize(STFT, 0.0);
    }
    let mut pads = STFT / HOP + 2;
    while pads > 0 {
        pads -= 1;
        let base = acc[0].len();
        add_norm(&mut norm, &win, base);
        for c in 0..channels {
            for i in 0..STFT {
                frame[i] = Complex32::new(head[c][i] * win[i], 0.0);
            }
            forward.process(&mut frame);
            for (b, v) in frame.iter_mut().take(bins).enumerate() {
                *v *= gain[b];
            }
            for v in frame.iter_mut().skip(bins) {
                *v = Complex32::default();
            }
            inverse.process(&mut frame);
            let scale = 1.0 / STFT as f32;
            let lane = &mut acc[c];
            for i in 0..STFT {
                let y = frame[i].re * scale * win[i];
                if i < lane.len() {
                    lane[i] += y;
                } else {
                    lane.push(y);
                }
            }
            let lane = &mut head[c];
            lane.drain(..HOP);
            lane.resize(STFT, 0.0);
        }
        let take = HOP.min(norm.len());
        emit(
            take,
            channels,
            &mut acc,
            &mut norm,
            &mut interleaved,
            &mut data_samples,
            &mut w,
        )?;
    }
    if !norm.is_empty() {
        let take = norm.len();
        emit(
            take,
            channels,
            &mut acc,
            &mut norm,
            &mut interleaved,
            &mut data_samples,
            &mut w,
        )?;
    }
    w.flush().map_err(|e| e.to_string())?;
    drop(w);
    let _ = child.wait();

    let mut patch = std::fs::OpenOptions::new()
        .write(true)
        .open(out)
        .map_err(|e| e.to_string())?;
    write_wav_header(&mut patch, channels, data_samples).map_err(|e| e.to_string())?;
    Ok(data_samples as u64)
}

/// Pick the band to cut across all reports: the union of detected spans, or
/// the widest evidence when tracks disagree.
pub fn choose_band(reports: &[TrackReport]) -> Option<(f32, f32)> {
    let mut lo = f32::MAX;
    let mut hi = 0.0f32;
    for r in reports {
        if let Some((a, b)) = r.band {
            lo = lo.min(a);
            hi = hi.max(b);
        }
    }
    (hi > lo).then(|| (lo.max(200.0), hi.min(FULL_BAND.1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detrend_finds_a_ridge() {
        let mut db = vec![-60.0f32; 400];
        db[200] = -30.0;
        let ex = detrend(&db);
        assert!(ex[200] > 20.0, "ridge not isolated: {}", ex[200]);
        assert!(ex[100].abs() < 1.0);
    }

    #[test]
    fn ridge_count_is_band_limited() {
        let hz = |i: usize| i as f32 * bin_hz();
        let mut ex = vec![0.0f32; FFT / 2 + 1];
        ex[120] = 9.0; // ~350 Hz, in band
        ex[1200] = 9.0; // ~3.5 kHz, out of the ridge band
        assert_eq!(count_ridges(&ex, hz, RIDGE_BAND), 1);
        assert_eq!(count_ridges(&ex, hz, FULL_BAND), 2);
    }

    #[test]
    fn verdict_thresholds() {
        assert_eq!(
            Verdict::of(0, 400.0, 40.0, 60.0),
            Verdict::Clean,
            "no ridges"
        );
        assert_eq!(
            Verdict::of(LIKELY, 2.0, 40.0, 60.0),
            Verdict::Clean,
            "weak ridges"
        );
        assert_eq!(
            Verdict::of(LIKELY, 20.0, 20.0, 60.0),
            Verdict::Clean,
            "quiet band"
        );
        assert_eq!(Verdict::of(LIKELY, 20.0, 40.0, 60.0), Verdict::Likely);
        assert_eq!(
            Verdict::of(STRONG, MASS_STRONG, 40.0, 1.0),
            Verdict::Likely,
            "no modulation"
        );
        assert_eq!(
            Verdict::of(STRONG, MASS_STRONG, 40.0, 60.0),
            Verdict::Strong
        );
    }

    #[test]
    fn atmos_keeps_the_bed_lossless() {
        // Auto must not fold a 7.1 Atmos bed to 5.1 TrueHD; FLAC keeps it.
        let t = target_for(Codec::Auto, Codec::TrueHd, false, true, None, 8, true).unwrap();
        assert_eq!(t.encoder, "flac");
        assert!(t.note.unwrap().contains("Atmos"));
        // An explicit choice still loses the objects, and says so.
        let t = target_for(Codec::Eac3, Codec::TrueHd, false, true, None, 8, true).unwrap();
        assert_eq!(t.encoder, "eac3");
        assert_eq!(t.channels, 8);
        assert!(t.note.unwrap().contains("objects are dropped"));
    }

    #[test]
    fn source_format_is_followed() {
        let t = target_for(Codec::Auto, Codec::Ac3, false, false, Some(448), 6, true).unwrap();
        assert_eq!(t.encoder, "ac3");
        assert_eq!(t.bitrate, Some(448));
        let t = target_for(Codec::Auto, Codec::Dts, true, false, None, 6, true).unwrap();
        assert_eq!(t.encoder, "flac", "DTS-HD has no encoder");
        let t = target_for(Codec::Auto, Codec::Dts, false, false, None, 6, true).unwrap();
        assert_eq!(t.encoder, "dca");
        assert!(t.strict);
    }

    #[test]
    fn gain_curve_is_flat_outside_the_band() {
        let spec = BandSpec::default_for(DEFAULT_BAND);
        let g = gain_curve(spec);
        assert!((g[0] - 1.0).abs() < 1e-6);
        assert!(
            (g[STFT / 2] - 1.0).abs() < 1e-6,
            "top of spectrum should be untouched"
        );
        assert!(
            (g[10] - 2.0).abs() < 1e-6,
            "mirror folding doubles the untouched bins"
        );
        let mid = (400.0 / (RATE as f32 / STFT as f32)) as usize;
        assert!(g[mid] < 1.0 && g[mid] > 0.0, "in-band gain should be down");
    }

    #[test]
    fn ridge_span_stops_at_a_wide_gap() {
        let hz = |i: usize| i as f32 * bin_hz();
        let mut ex = vec![0.0f32; FFT / 2 + 1];
        for i in 100..160 {
            ex[i] = 8.0;
        }
        for i in 400..460 {
            ex[i] = 8.0;
        }
        let (lo, hi) = ridge_span(&ex, hz);
        assert!(
            lo < 300.0 && hi < 600.0,
            "span {lo}..{hi} should not bridge the gap"
        );
    }
}
