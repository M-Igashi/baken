//! Waveform sections computed from decoded audio.
//!
//! Every rule here is kept because it reproduces rekordbox 7's own analysis
//! files of the audio it was run on.
//!
//! The blue scrolling waveform (`PWV3`) was recovered by comparing 1070
//! analysis files with their audio (issue #147): height and whiteness are
//! rekordbox's on 99.5% of columns. Its grid is 150 columns per second,
//! column `i` covering samples `[round(i * rate / 150), round((i + 1) * rate
//! / 150))` of the mono mix `(L + R) / 2`.
//!
//! The colour and three-band sections (`PWV5`, `PWV4`, `PWV7`, `PWV6`,
//! `PWVC`) follow rekordbox's own arithmetic as rbxport documented it
//! (`rekordbox-waveforms.md`; facts only, no code taken), checked here
//! against rekordbox's files of 40 tracks in five formats (issue #377):
//! `PWV5` and `PWV7` are identical on 99.5% of entries or more, `PWV6` on 97%
//! and within one everywhere, `PWVC` on 39 tracks, the first three bytes of
//! `PWV4` on 93 to 100%. They work on milliseconds: each keeps the peak of
//! ten filtered bands and of the raw signal, and the sections lay their
//! points over the milliseconds ([`points`]). The last three bytes of `PWV4`
//! are the closest reading of that description, not rekordbox's values (mean
//! error 3, 9 and 12 of 127), and the blue previews (`PWAV`, `PWV2`) are still
//! this crate's own approximations.
//!
//! Tracks are measured as they are decoded ([`Meter`]), so only the columns
//! and milliseconds are held: about 33 KB for a second of audio.

use crate::anlz::section::{section, Section};
use baken_core::decode::decode_with;
use std::path::Path;

/// Preview and detail waveforms of one track.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Waveforms {
    /// `PWV3`: 5-bit height, 3-bit whiteness per 1/150 s.
    pub pwv3: Vec<u8>,
    /// `PWV5`: rrrgggbbbhhhhh00 per 1/150 s.
    pub pwv5: Vec<u16>,
    /// `PWV7`: low, mid, high per 1/150 s.
    pub pwv7: Vec<[u8; 3]>,
    /// `PWVC`: low, mid, high gain in hundredths.
    pub gains: [u16; 3],
    /// `PWAV`: 400 columns.
    pub pwav: Vec<u8>,
    /// `PWV2`: 100 columns, 4-bit height.
    pub pwv2: Vec<u8>,
    /// `PWV6`: 1200 columns of low, mid, high.
    pub pwv6: Vec<[u8; 3]>,
    /// `PWV4`: 1200 columns of six bytes.
    pub pwv4: Vec<[u8; 6]>,
}

const COLUMNS_PER_SECOND: f64 = 150.0;
const PREVIEW_COLUMNS: usize = 400;
const TINY_COLUMNS: usize = 100;
const COLOUR_COLUMNS: usize = 1200;

/// Second-order Butterworth low-pass whose peak, relative to the column peak,
/// is the whiteness.
const WHITENESS_LOWPASS_HZ: f64 = 150.0;
/// rekordbox never writes a preview column lower than these: on the
/// reference stick every `PWAV` height is at least 2 and every `PWV2` byte
/// at least 1 (587 of 587 `.DAT` files). Nexus-era firmware reads a zero
/// column as an unfinished preview and draws its own (issue #285).
const PWAV_MIN_HEIGHT: u8 = 2;
const PWV2_MIN_HEIGHT: u8 = 1;
/// What rekordbox writes for its quietest `PWAV` columns: height 2,
/// whiteness 5 (8449 of the 9809 height-2 columns on the reference stick).
const PWAV_FLOOR: u8 = (5 << 5) | PWAV_MIN_HEIGHT;

/// `PWVC` never goes below 0.80 or above these (low, mid, high), in hundredths.
const GAIN_FLOOR: u16 = 80;
const GAIN_CAP: [f64; 3] = [3.0, 3.0, 5.0];
/// How far back the `PWV7` envelope of each band starts, in milliseconds, and
/// how much of the gap to a lower value it keeps per millisecond.
const ENVELOPE_MS: [usize; 3] = [300, 200, 100];
const ENVELOPE_KEEP: [f64; 3] = [0.99, 0.98, 0.97];
/// Milliseconds either side of a point that its low, mid and high colour
/// take in.
const COLOUR_WIDEN: [usize; 3] = [12, 1, 0];
/// `PWV5` height: `trunc(trunc(peak * 32767 / loudest)^2 * this)`.
const HEIGHT_SCALE: f64 = 2.9327451233027466e-8;
/// `PWV5` and `PWV7` of silence, as rekordbox writes them.
const PWV5_SILENCE: u16 = 0xff80;

/// Direct form I biquad.
#[derive(Clone, Copy)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Biquad {
    fn new(b: [f64; 3], a0: f64, a1: f64, a2: f64) -> Self {
        Biquad {
            b: [b[0] / a0, b[1] / a0, b[2] / a0],
            a: [a1 / a0, a2 / a0],
            x: [0.0; 2],
            y: [0.0; 2],
        }
    }

    /// RBJ cookbook low-pass, Q = 1/sqrt(2).
    fn lowpass(fc: f64, rate: f64) -> Self {
        let w = 2.0 * std::f64::consts::PI * fc / rate;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        Self::new(
            [(1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0],
            1.0 + alpha,
            -2.0 * c,
            1.0 - alpha,
        )
    }

    fn highpass(fc: f64, rate: f64) -> Self {
        let w = 2.0 * std::f64::consts::PI * fc / rate;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        Self::new(
            [(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0],
            1.0 + alpha,
            -2.0 * c,
            1.0 - alpha,
        )
    }

    #[inline]
    fn step(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1]
            - self.a[0] * self.y[0]
            - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// Per-column measurements at 150 columns per second.
#[derive(Debug, Default, Clone)]
pub(crate) struct Column {
    peak: f64,
    white: f64,
    sumsq: f64,
    samples: u32,
}

/// The bands rekordbox filters the signal into, each a chain of RBJ biquads
/// at Q = 1/sqrt(2): the three-band low, mid and high (`PWV7`, `PWV6`,
/// `PWVC`), the bass and the colour low, mid and high of `PWV4`, and the
/// detail low, mid and high of `PWV5`.
const BANDS: usize = 10;
const BASS: usize = 3;
const COLOUR: usize = 4;
const DETAIL: usize = 7;

fn band_filters(rate: f64) -> [Vec<Biquad>; BANDS] {
    let (lp, hp) = (
        |f: f64| Biquad::lowpass(f, rate),
        |f: f64| Biquad::highpass(f, rate),
    );
    [
        vec![lp(300.0)],
        vec![hp(250.0), lp(1200.0)],
        vec![hp(3000.0), lp(9000.0)],
        vec![lp(400.0)],
        vec![lp(200.0), lp(200.0)],
        vec![hp(200.0), hp(200.0), lp(2000.0), lp(2000.0)],
        vec![hp(2000.0), hp(2000.0)],
        vec![lp(100.0)],
        vec![hp(300.0), lp(3000.0)],
        vec![hp(1500.0)],
    ]
}

/// A value on rekordbox's 16-bit scale: truncated, not rounded.
fn sixteen_bit(x: f64) -> i32 {
    ((x * 32768.0).trunc() as i64).clamp(-32768, 32767) as i32
}

/// One millisecond of the track: the highest and lowest sample, and the peak
/// of each band. A colour band's peak is weighted by the share of the
/// samples in which it was the strongest of the three, and is full scale
/// where the band reached full scale.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Milli {
    high: i16,
    low: i16,
    band: [i16; BANDS],
}

impl Milli {
    fn peak(&self) -> i32 {
        i32::from(self.high).abs().max(i32::from(self.low).abs())
    }
}

/// The milliseconds of a track, kept in blocks of one size: a whole track in
/// one buffer is megabytes of a different size each time, which the
/// allocator holds on to track after track instead of using again (8 MB a
/// track on macOS, measured; issue #171 was the same with the audio).
#[derive(Debug, Clone, Default)]
pub(crate) struct Millis {
    blocks: Vec<Vec<Milli>>,
    len: usize,
}

const MILLIS_BLOCK: usize = 4096;

impl Millis {
    fn push(&mut self, ms: Milli) {
        if self.len.is_multiple_of(MILLIS_BLOCK) {
            self.blocks.push(Vec::with_capacity(MILLIS_BLOCK));
        }
        self.blocks[self.len / MILLIS_BLOCK].push(ms);
        self.len += 1;
    }

    fn len(&self) -> usize {
        self.len
    }

    fn get(&self, i: usize) -> Milli {
        self.blocks[i / MILLIS_BLOCK][i % MILLIS_BLOCK]
    }

    /// The milliseconds `a..=b`.
    fn range(&self, a: usize, b: usize) -> impl Iterator<Item = Milli> + '_ {
        (a..=b).map(|i| self.get(i))
    }

    fn iter(&self) -> impl Iterator<Item = Milli> + '_ {
        self.blocks.iter().flatten().copied()
    }
}

/// The millisecond being measured.
#[derive(Clone, Copy, Default)]
struct Running {
    samples: u32,
    high: i32,
    low: i32,
    peak: [i32; BANDS],
    led: [u32; 3],
    full: [bool; 3],
}

impl Running {
    fn close(&self) -> Milli {
        let mut band = self.peak;
        for j in 0..3 {
            band[COLOUR + j] = if self.full[j] {
                32767
            } else {
                (f64::from(band[COLOUR + j]) * f64::from(self.led[j])
                    / f64::from(self.samples.max(1))) as i32
            };
        }
        Milli {
            high: self.high as i16,
            low: self.low as i16,
            band: band.map(|v| v.min(32767) as i16),
        }
    }
}

/// A decoded track reduced to its columns and milliseconds and the numbers
/// the rest of the analysis needs.
#[derive(Debug, Clone, Default)]
pub struct Measured {
    pub sample_rate: u32,
    pub channels: usize,
    pub frames: u64,
    pub(crate) columns: Vec<Column>,
    pub(crate) millis: Millis,
}

impl Measured {
    pub fn duration_ms(&self) -> f64 {
        self.frames as f64 * 1000.0 / self.sample_rate.max(1) as f64
    }
}

/// Measures interleaved audio into columns and milliseconds as it arrives.
pub struct Meter {
    sample_rate: u32,
    rate: f64,
    channels: usize,
    frames: u64,
    /// Finished columns; `current` is the one being filled.
    columns: Vec<Column>,
    current: Column,
    next_edge: u64,
    /// Whiteness low-pass of the blue waveform.
    white: Biquad,
    /// Finished milliseconds; `running` is the one being filled.
    millis: Millis,
    running: Running,
    bands: [Vec<Biquad>; BANDS],
}

impl Meter {
    pub fn new(sample_rate: u32, channels: usize) -> Self {
        let rate = sample_rate.max(1) as f64;
        Meter {
            sample_rate,
            rate,
            channels,
            frames: 0,
            columns: Vec::new(),
            current: Column::default(),
            next_edge: (rate / COLUMNS_PER_SECOND).round() as u64,
            white: Biquad::lowpass(WHITENESS_LOWPASS_HZ, rate),
            millis: Millis::default(),
            running: Running::default(),
            bands: band_filters(rate),
        }
    }

    pub fn push(&mut self, interleaved: &[f32]) {
        let channels = self.channels.max(1);
        let rate = u64::from(self.sample_rate.max(1));
        // locals, so the per-sample state stays in registers
        let mut white = self.white;
        let (mut frames, mut next_edge) = (self.frames, self.next_edge);
        let mut c = std::mem::take(&mut self.current);
        let mut ms = self.running;
        for frame in interleaved.chunks_exact(channels) {
            while frames >= next_edge {
                self.columns.push(std::mem::take(&mut c));
                next_edge = ((self.columns.len() + 1) as f64 * self.rate / COLUMNS_PER_SECOND)
                    .round() as u64;
            }
            // millisecond `i` holds the frames from `ceil(i * rate / 1000)` on
            while (frames * 1000 / rate) as usize > self.millis.len() {
                self.millis.push(ms.close());
                ms = Running::default();
            }
            // A NaN would stay in the filters' state for the rest of the track.
            let clean = |s: f32| if s.is_finite() { f64::from(s) } else { 0.0 };
            let x = frame.iter().map(|&s| clean(s)).sum::<f64>() / channels as f64;
            c.peak = c.peak.max(x.abs());
            c.white = c.white.max(white.step(x).abs());
            c.sumsq += x * x;
            c.samples += 1;

            let m = rekordbox_mono(frame, x, clean);
            let raw = sixteen_bit(m);
            (ms.high, ms.low) = if ms.samples == 0 {
                (raw, raw)
            } else {
                (ms.high.max(raw), ms.low.min(raw))
            };
            let mut out = [0i32; BANDS];
            for (band, chain) in self.bands.iter_mut().enumerate() {
                let y = chain.iter_mut().fold(m, |y, f| f.step(y));
                out[band] = sixteen_bit(y).abs();
                ms.peak[band] = ms.peak[band].max(out[band]);
            }
            let colour = &out[COLOUR..COLOUR + 3];
            let leader = (0..3).max_by_key(|&j| colour[j]).unwrap_or(0);
            ms.led[leader] += 1;
            for j in 0..3 {
                ms.full[j] |= colour[j] >= 32767;
            }
            ms.samples += 1;
            frames += 1;
        }
        self.current = c;
        self.white = white;
        self.running = ms;
        (self.frames, self.next_edge) = (frames, next_edge);
    }

    /// `ceil(frames * 150 / rate)` columns: a last column that starts at
    /// the end of the audio stays empty. `ceil(frames * 1000 / rate)`
    /// milliseconds.
    pub fn finish(mut self) -> Measured {
        let n = ((self.frames as f64 * COLUMNS_PER_SECOND / self.rate).ceil() as usize).max(1);
        self.columns.push(self.current);
        debug_assert!(self.columns.len() <= n);
        self.columns.resize(n, Column::default());
        if self.running.samples > 0 {
            self.millis.push(self.running.close());
        }
        Measured {
            sample_rate: self.sample_rate,
            channels: self.channels,
            frames: self.frames,
            columns: self.columns,
            millis: self.millis,
        }
    }
}

/// The mono value rekordbox's colour and three-band analysis works on: of a
/// stereo frame brought to 16 bits, the louder channel where the two are
/// within 0.001 of each other in level (the right one on a tie), so that
/// channels of opposite sign do not cancel, and their mean otherwise. `mean`
/// is the mean of all channels, used for anything but stereo.
fn rekordbox_mono(frame: &[f32], mean: f64, clean: impl Fn(f32) -> f64) -> f64 {
    let [l, r] = frame else {
        return mean;
    };
    let (l, r) = (
        f64::from(sixteen_bit(clean(*l))) / 32767.0,
        f64::from(sixteen_bit(clean(*r))) / 32767.0,
    );
    if (l.abs() - r.abs()).abs() >= 0.001 {
        (l + r) / 2.0
    } else if l.abs() > r.abs() {
        l
    } else {
        r
    }
}

/// Decode `path` and measure it on the way.
pub fn measure(path: &Path) -> anyhow::Result<Measured> {
    measure_delayed(path, 0.0)
}

/// [`measure`] with `delay` seconds of silence before the audio, for a
/// `--cdjsafe` MP3 that plays that much later than `path` (issue #374).
pub fn measure_delayed(path: &Path, delay: f64) -> anyhow::Result<Measured> {
    let file = std::fs::File::open(path)?;
    let extension = path.extension().and_then(|e| e.to_str());
    let mut meter: Option<Meter> = None;
    decode_with(file, extension, |chunk, rate, channels| {
        meter
            .get_or_insert_with(|| {
                let mut meter = Meter::new(rate, channels);
                let frames = (delay.max(0.0) * f64::from(rate)).round() as usize;
                meter.push(&vec![0.0; frames * channels]);
                meter
            })
            .push(chunk)
    })?;
    Ok(meter.map(Meter::finish).unwrap_or_default())
}

/// `floor(31.5 * (peak / track peak)^2)`, rekordbox's height rule.
fn height(peak: f64, track_peak: f64) -> u8 {
    if track_peak <= 0.0 {
        return 0;
    }
    let q = peak / track_peak;
    ((31.5 * q * q).floor() as u8).min(31)
}

/// `7 - floor(8 * lowpassed peak / peak)`; silence counts as white.
fn whiteness(low: f64, peak: f64) -> u8 {
    if peak <= 0.0 {
        return 7;
    }
    let r = (low / peak).clamp(0.0, 1.0);
    (7 - ((8.0 * r).floor() as i32).min(7)) as u8
}

/// Mean of `values[a..b]` with `b > a` guaranteed by the caller.
fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

/// Split `n` items into `parts` consecutive ranges, each at least one item
/// long (ranges overlap when `n < parts`).
fn ranges(n: usize, parts: usize) -> impl Iterator<Item = std::ops::Range<usize>> {
    (0..parts).map(move |k| {
        let a = (k * n / parts).min(n - 1);
        let b = ((k + 1) * n / parts).clamp(a + 1, n);
        a..b
    })
}

pub fn analyze(audio: &Measured) -> Waveforms {
    let columns = &audio.columns;
    let n = columns.len();
    let track_peak = columns.iter().map(|c| c.peak).fold(0.0, f64::max);

    let heights: Vec<u8> = columns.iter().map(|c| height(c.peak, track_peak)).collect();
    let whites: Vec<u8> = columns.iter().map(|c| whiteness(c.white, c.peak)).collect();
    let pwv3: Vec<u8> = heights
        .iter()
        .zip(&whites)
        .map(|(h, w)| (w << 5) | h)
        .collect();

    // Previews from RMS per column group.
    let rms_over = |r: std::ops::Range<usize>| -> f64 {
        let (sumsq, samples) = columns[r]
            .iter()
            .fold((0.0, 0u32), |(s, k), c| (s + c.sumsq, k + c.samples));
        if samples == 0 {
            0.0
        } else {
            (sumsq / samples as f64).sqrt()
        }
    };
    let rms400: Vec<f64> = ranges(n, PREVIEW_COLUMNS).map(rms_over).collect();
    let rms_max = rms400.iter().cloned().fold(0.0, f64::max);
    let whites_f: Vec<f64> = whites.iter().map(|&w| w as f64).collect();
    let white200: Vec<u8> = ranges(n, PREVIEW_COLUMNS / 2)
        .map(|r| mean(&whites_f[r]).round() as u8)
        .collect();
    let pwav: Vec<u8> = rms400
        .iter()
        .enumerate()
        .map(|(k, &rms)| {
            let h = if rms_max > 0.0 {
                (24.0 * rms / rms_max).round() as u8
            } else {
                0
            };
            if h < PWAV_MIN_HEIGHT {
                PWAV_FLOOR
            } else {
                (white200[k / 2].min(7) << 5) | h.min(31)
            }
        })
        .collect();
    let rms100: Vec<f64> = ranges(n, TINY_COLUMNS).map(rms_over).collect();
    let rms100_max = rms100.iter().cloned().fold(0.0, f64::max);
    let pwv2: Vec<u8> = rms100
        .iter()
        .map(|&rms| {
            let h = if rms100_max > 0.0 {
                ((15.0 * (rms / rms100_max).sqrt()).round() as u8).min(15)
            } else {
                0
            };
            h.max(PWV2_MIN_HEIGHT)
        })
        .collect();

    let millis = &audio.millis;
    let pwv5 = colour_detail(millis, n);
    let pwv4 = colour_preview(millis);
    let (gains, pwv6, pwv7) = three_band(millis, n);

    Waveforms {
        pwv3,
        pwv5,
        pwv7,
        gains,
        pwav,
        pwv2,
        pwv6,
        pwv4,
    }
}

/// The first and last millisecond of each of `n` points laid over `ms`
/// milliseconds, the way rekordbox spreads them: a counter gains `n` every
/// millisecond and closes a point each time it reaches `ms`.
fn points(ms: usize, n: usize) -> impl Iterator<Item = (usize, usize)> {
    // `taken` milliseconds are in; `first` is where the open point starts
    let (mut counter, mut first, mut taken) = (0, 0, 0);
    std::iter::from_fn(move || {
        while counter < ms || ms == 0 {
            if taken == ms {
                return None;
            }
            counter += n;
            taken += 1;
        }
        counter -= ms;
        let last = taken - 1;
        let point = (first.min(last), last);
        first = taken;
        Some(point)
    })
}

/// The peak of `band` over the milliseconds `a..=b`, widened by `widen` on
/// both sides.
fn band_peak(millis: &Millis, band: usize, (a, b): (usize, usize), widen: usize) -> f64 {
    let (a, b) = (a.saturating_sub(widen), (b + widen).min(millis.len() - 1));
    f64::from(millis.range(a, b).map(|m| m.band[band]).max().unwrap_or(0))
}

/// `PWV5`, `rrrgggbbbhhhhh00` per point: the height on the square law of the
/// blue waveform, from the point's largest sample against the track's, and
/// the colour from the detail bands' peaks, brought to 255 for the strongest
/// and then tempered the way rekordbox does it.
fn colour_detail(millis: &Millis, n: usize) -> Vec<u16> {
    if millis.len() == 0 {
        return vec![PWV5_SILENCE; n];
    }
    let loudest = millis.iter().map(|m| m.peak()).max().unwrap_or(0).max(1);
    points(millis.len(), n)
        .map(|(a, b)| {
            let peak = millis.range(a, b).map(|m| m.peak()).max().unwrap_or(0);
            let scaled = (f64::from(peak) * 32767.0 / f64::from(loudest)).trunc();
            let height = ((scaled * scaled * HEIGHT_SCALE).trunc() as u16).min(31);
            let band: [f64; 3] =
                std::array::from_fn(|j| band_peak(millis, DETAIL + j, (a, b), COLOUR_WIDEN[j]));
            let top = band.iter().cloned().fold(0.0, f64::max);
            if top <= 0.0 {
                return PWV5_SILENCE | (height << 2);
            }
            let [mut r, mut g, mut bl] = band.map(|v| v * 255.0 / top);
            if bl < 64.0 {
                let cut = g * r / 765.0 * (1.0 - bl / 64.0);
                r -= cut;
                g -= cut;
            }
            g *= 1.0 - 0.00234375 * r.max(bl).min(128.0);
            bl *= 1.3;
            let level = |v: f64| (v.clamp(0.0, 255.0) as u16) >> 5;
            let mut rgb = [level(r), level(g), level(bl)];
            if r < 32.0 && g < 32.0 && bl < 32.0 {
                // too faint for any level: the band that led shows alone
                let leader = if band[0] >= band[1] && band[0] >= band[2] {
                    0
                } else if band[1] >= band[2] {
                    1
                } else {
                    2
                };
                rgb = std::array::from_fn(|j| u16::from(j == leader));
            }
            (rgb[0] << 13) | (rgb[1] << 10) | (rgb[2] << 7) | (height << 2)
        })
        .collect()
}

/// `PWV4`, six signed bytes for each of 1200 columns, each a 16-bit value
/// over 256: the highest and the lowest sample, the bass peak, and the
/// colour bands' peaks.
fn colour_preview(millis: &Millis) -> Vec<[u8; 6]> {
    if millis.len() == 0 {
        return vec![[0; 6]; COLOUR_COLUMNS];
    }
    let byte = |v: i32| ((v / 256) as i8) as u8;
    points(millis.len(), COLOUR_COLUMNS)
        .map(|(a, b)| {
            let high = millis.range(a, b).map(|m| m.high).max().unwrap_or(0);
            let low = millis.range(a, b).map(|m| m.low).min().unwrap_or(0);
            let colour = |j: usize| band_peak(millis, COLOUR + j, (a, b), COLOUR_WIDEN[j]) as i32;
            [
                byte(i32::from(high)),
                byte(i32::from(low)),
                byte(band_peak(millis, BASS, (a, b), 0) as i32),
                byte(colour(0)),
                byte(colour(1)),
                byte(colour(2)),
            ]
        })
        .collect()
}

/// `PWVC`, `PWV6` and `PWV7`: the gain of each band, the 1200-column preview
/// and the detail.
///
/// The gain comes from the preview: each column's mean of a band, taking in
/// a lead before the column (a thousandth of the track for low, two thirds
/// of that for mid, a third for high), the three bands brought to equal sums
/// and the tallest column of their total to full scale. It is then held to
/// what the band's own peak allows before it is stored. The detail runs an
/// envelope up to each point that rises at once and falls slowly.
fn three_band(millis: &Millis, n: usize) -> ([u16; 3], Vec<[u8; 3]>, Vec<[u8; 3]>) {
    let m = millis.len();
    if m == 0 {
        return (
            [GAIN_FLOOR; 3],
            vec![[0; 3]; COLOUR_COLUMNS],
            vec![[0; 3]; n],
        );
    }
    let lead = [
        m as f64 / 1000.0,
        2.0 * (m as f64 / 1000.0) / 3.0,
        m as f64 / 3000.0,
    ]
    .map(|ms| ms.round() as usize);
    // what lies before the start counts as silence
    let mean = |j: usize, (a, b): (usize, usize)| {
        let sum: f64 = millis
            .range(a.saturating_sub(lead[j]), b)
            .map(|ms| f64::from(ms.band[j]))
            .sum();
        sum / (b + lead[j] + 1 - a) as f64
    };
    let means: Vec<[f64; 3]> = points(m, COLOUR_COLUMNS)
        .map(|column| std::array::from_fn(|j| mean(j, column)))
        .collect();
    let totals: [f64; 3] = std::array::from_fn(|j| means.iter().map(|c| c[j]).sum());
    let largest = totals.iter().cloned().fold(0.0, f64::max);
    let equal: [f64; 3] = totals.map(|t| if t > 0.0 { largest / t } else { 0.0 });
    let tallest = means
        .iter()
        .map(|c| (0..3).map(|j| c[j] * equal[j]).sum::<f64>())
        .fold(0.0, f64::max);
    let raw = equal.map(|e| {
        if tallest > 0.0 {
            32768.0 * e / tallest
        } else {
            0.0
        }
    });
    let peaks: [f64; 3] =
        std::array::from_fn(|j| f64::from(millis.iter().map(|ms| ms.band[j]).max().unwrap_or(0)));
    // the high band is stored on a cosine curve, the others straight
    let high_curve = |level: f64| 1.0 - (std::f64::consts::PI * level / 32768.0).cos();
    let gains: [u16; 3] = std::array::from_fn(|j| {
        let room = match j {
            2 => 32768.0 / (16384.0 * high_curve(peaks[j])).max(f64::MIN_POSITIVE),
            _ => 32768.0 / peaks[j].max(1.0),
        };
        match raw[j].min(room) {
            held if held < 0.8 => GAIN_FLOOR,
            held => (100.0 * held.min(GAIN_CAP[j])).trunc() as u16,
        }
    });
    let byte = |v: f64| v.trunc().min(255.0) as u8;
    let pwv6 = means
        .iter()
        .map(|c| std::array::from_fn(|j| byte(c[j] * raw[j].clamp(0.8, GAIN_CAP[j]) / 256.0)))
        .collect();
    let pwv7 = points(m, n)
        .map(|(_, last)| {
            std::array::from_fn(|j| {
                let from = last.saturating_sub(ENVELOPE_MS[j]);
                let level = millis.range(from, last).fold(0.0, |level, ms| {
                    let now = f64::from(ms.band[j]);
                    if now > level {
                        now
                    } else {
                        now + (level - now) * ENVELOPE_KEEP[j]
                    }
                });
                let gain = f64::from(gains[j]) / 100.0;
                match j {
                    2 => byte(gain * 64.0 * high_curve(level)),
                    _ => byte(level * gain / 256.0),
                }
            })
        })
        .collect();
    (gains, pwv6, pwv7)
}

fn be32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

impl Waveforms {
    /// `PWAV` and `PWV2` for the `.DAT` file, in rekordbox's order.
    pub fn dat_sections(&self) -> Vec<Section> {
        vec![
            preview_section(b"PWAV", &self.pwav),
            preview_section(b"PWV2", &self.pwv2),
        ]
    }

    /// `PWV3`, `PWV5` and `PWV4` for the `.EXT` file (callers place `PWV3`
    /// first and the other two after the cue and grid sections).
    pub fn pwv3_section(&self) -> Section {
        detail_section(b"PWV3", 1, 0x0096_0000, &self.pwv3)
    }

    pub fn pwv5_section(&self) -> Section {
        let bytes: Vec<u8> = self.pwv5.iter().flat_map(|v| v.to_be_bytes()).collect();
        detail_section(b"PWV5", 2, 0x0096_0305, &bytes)
    }

    pub fn pwv4_section(&self) -> Section {
        let bytes: Vec<u8> = self.pwv4.iter().flatten().copied().collect();
        detail_section(b"PWV4", 6, 0, &bytes)
    }

    /// `PWV7`, `PWV6` and `PWVC` for the `.2EX` file, in rekordbox's order.
    pub fn two_ex_sections(&self) -> Vec<Section> {
        let pwv7: Vec<u8> = self.pwv7.iter().flatten().copied().collect();
        let pwv6: Vec<u8> = self.pwv6.iter().flatten().copied().collect();
        let mut p6 = Vec::with_capacity(8 + pwv6.len());
        p6.extend_from_slice(&be32(3));
        p6.extend_from_slice(&be32(self.pwv6.len() as u32));
        p6.extend_from_slice(&pwv6);
        let mut pc = Vec::with_capacity(8);
        pc.extend_from_slice(&0u16.to_be_bytes());
        for g in self.gains {
            pc.extend_from_slice(&g.to_be_bytes());
        }
        vec![
            detail_section(b"PWV7", 3, 0x0096_0000, &pwv7),
            section(b"PWV6", 0x14, &p6),
            section(b"PWVC", 0x0e, &pc),
        ]
    }
}

/// `len_data, 0x00010000, data` with a 20-byte header (`PWAV`, `PWV2`).
fn preview_section(tag: &[u8; 4], data: &[u8]) -> Section {
    let mut p = Vec::with_capacity(8 + data.len());
    p.extend_from_slice(&be32(data.len() as u32));
    p.extend_from_slice(&be32(0x0001_0000));
    p.extend_from_slice(data);
    section(tag, 0x14, &p)
}

/// `len_entry_bytes, len_entries, unknown, data` with a 24-byte header.
fn detail_section(tag: &[u8; 4], entry_bytes: u32, unknown: u32, data: &[u8]) -> Section {
    let mut p = Vec::with_capacity(12 + data.len());
    p.extend_from_slice(&be32(entry_bytes));
    p.extend_from_slice(&be32(data.len() as u32 / entry_bytes));
    p.extend_from_slice(&be32(unknown));
    p.extend_from_slice(data);
    section(tag, 0x18, &p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone_samples(rate: u32, seconds: f64, hz: f64, amp: f64) -> Vec<f32> {
        let frames = (rate as f64 * seconds) as usize;
        (0..frames)
            .flat_map(|i| {
                let v =
                    (amp * (2.0 * std::f64::consts::PI * hz * i as f64 / rate as f64).sin()) as f32;
                [v, v]
            })
            .collect()
    }

    fn metered(rate: u32, channels: usize, chunks: &[&[f32]]) -> Measured {
        let mut m = Meter::new(rate, channels);
        for c in chunks {
            m.push(c);
        }
        m.finish()
    }

    fn tone(rate: u32, seconds: f64, hz: f64, amp: f64) -> Measured {
        metered(rate, 2, &[&tone_samples(rate, seconds, hz, amp)])
    }

    #[test]
    fn column_count_and_section_sizes() {
        let w = analyze(&tone(44100, 2.0, 440.0, 0.5));
        assert_eq!(w.pwv3.len(), 300);
        assert_eq!(w.pwv5.len(), 300);
        assert_eq!(w.pwv7.len(), 300);
        assert_eq!(
            (w.pwav.len(), w.pwv2.len(), w.pwv6.len(), w.pwv4.len()),
            (400, 100, 1200, 1200)
        );
        let dat = w.dat_sections();
        assert_eq!((dat[0].bytes.len(), dat[1].bytes.len()), (420, 120));
        assert_eq!(w.pwv3_section().bytes.len(), 24 + 300);
        assert_eq!(w.pwv5_section().bytes.len(), 24 + 600);
        assert_eq!(w.pwv4_section().bytes.len(), 7224);
        let two = w.two_ex_sections();
        assert_eq!((two[1].bytes.len(), two[2].bytes.len()), (3620, 20));
        assert_eq!(&two[2].bytes[12..14], &[0, 0]);
    }

    #[test]
    fn steady_tone_is_full_height_and_bass_is_not_white() {
        let w = analyze(&tone(48000, 1.0, 1000.0, 0.8));
        let heights: Vec<u8> = w.pwv3[10..].iter().map(|b| b & 0x1f).collect();
        assert!(heights.iter().all(|&h| h >= 29), "{heights:?}");
        let bass = analyze(&tone(48000, 1.0, 60.0, 0.8));
        assert!(bass.pwv3[10..].iter().all(|b| b >> 5 <= 1));
        assert!(bass.pwv7[20][0] > bass.pwv7[20][2]);
        let bright = analyze(&tone(48000, 1.0, 8000.0, 0.8));
        assert!(bright.pwv3[10..].iter().all(|b| b >> 5 == 7));
        assert!(bright.pwv7[20][2] > bright.pwv7[20][0]);
    }

    /// Silence at both ends gives rekordbox's floor, never a zero column
    /// (issue #285), and the loud middle is untouched.
    #[test]
    fn silent_preview_columns_take_rekordbox_floor() {
        let rate = 44100;
        let silence = vec![0f32; rate as usize * 2];
        let w = metered(
            rate,
            2,
            &[&silence, &tone_samples(rate, 6.0, 440.0, 0.8), &silence],
        );
        let w = analyze(&w);
        assert!(w.pwav.iter().all(|&b| b & 0x1f >= 2), "{:?}", w.pwav);
        assert_eq!((w.pwav[0], w.pwav[399]), (0xa2, 0xa2));
        assert!(w.pwav[200] & 0x1f >= 20);
        assert!(
            w.pwv2.iter().all(|&b| (1..=15).contains(&b)),
            "{:?}",
            w.pwv2
        );
        assert_eq!((w.pwv2[0], w.pwv2[99]), (1, 1));

        let quiet = analyze(&metered(rate, 2, &[&silence]));
        assert!(quiet.pwav.iter().all(|&b| b == 0xa2));
        assert!(quiet.pwv2.iter().all(|&b| b == 1));
    }

    /// Decoders hand over buffers of any size; the result must not depend on them.
    #[test]
    fn chunked_input_measures_like_one_buffer() {
        for (rate, seconds) in [(44100, 1.3), (32000, 0.77), (22050, 0.5)] {
            let s = tone_samples(rate, seconds, 330.0, 0.6);
            let whole = analyze(&metered(rate, 2, &[&s]));
            let pieces: Vec<&[f32]> = s.chunks(2 * 777).collect();
            let chunked = metered(rate, 2, &pieces);
            assert_eq!(chunked.frames as usize, s.len() / 2);
            assert_eq!(analyze(&chunked), whole);
            let expected = (s.len() as f64 / 2.0 * 150.0 / rate as f64).ceil() as usize;
            assert_eq!(whole.pwv3.len(), expected);
        }
    }

    /// Every millisecond belongs to one point, in order, and a track shorter
    /// than its points repeats milliseconds instead of leaving points out.
    #[test]
    fn points_cover_the_milliseconds_once() {
        for (ms, n) in [(1000, 150), (6667, 1001), (300_123, 45_019), (7, 1200)] {
            let points: Vec<_> = points(ms, n).collect();
            assert_eq!(points.len(), n, "{ms} ms");
            assert_eq!((points[0].0, points[n - 1].1), (0, ms - 1));
            assert!(points
                .windows(2)
                .all(|w| w[1].0 <= w[0].1 + 1 && w[1].1 >= w[0].1));
            assert!(points.iter().all(|&(a, b)| a <= b));
        }
    }

    /// The same sample in both channels with opposite signs is one sample,
    /// not silence, and channels apart in level are averaged.
    #[test]
    fn the_colour_mix_keeps_channels_of_opposite_sign() {
        let clean = |s: f32| f64::from(s);
        assert!((rekordbox_mono(&[0.5, -0.5], 0.0, clean) + 0.5).abs() < 1e-3);
        assert!((rekordbox_mono(&[0.5, 0.1], 0.3, clean) - 0.3).abs() < 1e-3);
        assert_eq!(rekordbox_mono(&[0.25], 0.25, clean), 0.25);
        let mut meter = Meter::new(44100, 2);
        let inverted: Vec<f32> = tone_samples(44100, 1.0, 1000.0, 0.5)
            .chunks(2)
            .flat_map(|f| [f[0], -f[0]])
            .collect();
        meter.push(&inverted);
        let w = analyze(&meter.finish());
        assert!(
            w.pwv3.iter().all(|&b| b & 0x1f == 0),
            "the blue mix cancels"
        );
        assert!(
            w.pwv5[10..].iter().all(|&v| (v >> 2) & 0x1f >= 29),
            "{:?}",
            &w.pwv5[10..14]
        );
    }

    #[test]
    fn silence_is_zero() {
        let w = analyze(&metered(44100, 1, &[&[0.0; 44100]]));
        assert!(w.pwv3.iter().all(|&b| b == 0xe0));
        assert!(w.pwv7.iter().all(|b| *b == [0, 0, 0]));
        assert_eq!(w.gains, [GAIN_FLOOR; 3]);
        assert!(w.pwv5.iter().all(|&v| v == PWV5_SILENCE));
    }

    /// One NaN used to stay in the filters and blank the rest of the track
    /// (issue #330).
    #[test]
    fn a_nan_sample_does_not_blank_what_follows() {
        let mut samples = tone_samples(44100, 1.0, 1000.0, 0.8);
        samples[2000] = f32::NAN;
        samples[2001] = f32::NAN;
        let w = analyze(&metered(44100, 2, &[&samples]));
        let last = w.pwv7.last().unwrap();
        assert!(last.iter().all(|&band| band > 0), "{last:?}");
        assert!(w.pwv2.last().is_some_and(|&b| b > 1));
    }
}
