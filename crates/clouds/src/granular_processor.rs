//! `clouds/dsp/granular_processor.{h,cc}` -- the top-level processor: input
//! feedback path, optional 2x downsampling, the selected playback mode, then
//! the diffuser / pitch-shifter / filter / reverb post chain and the dry/wet
//! mix.

use alloc::boxed::Box;

use stmlib::constrain;
use stmlib::fdsp::{one_pole, soft_convert, soft_limit};
use stmlib::filter::{FilterMode, FrequencyApproximation, Svf};
use stmlib::units::semitones_to_ratio;
use stmlib::ParameterInterpolator;

use crate::audio_buffer::{AudioBuffer, Resolution};
use crate::correlator::Correlator;
use crate::dsp::interpolate;
use crate::frame::{FloatFrame, ShortFrame, MAX_BLOCK_SIZE};
use crate::fx::{Diffuser, PitchShifter, Reverb};
use crate::parameters::Parameters;
use crate::players::{GranularSamplePlayer, LoopingSamplePlayer, WSOLASamplePlayer};
use crate::resources::{LUT_XFADE_IN, LUT_XFADE_OUT, SRC_FILTER_1X_2_45};
use crate::sample_rate_converter::SampleRateConverter;

/// `kDownsamplingFactor`.
const DOWNSAMPLING_FACTOR: usize = 2;

/// `block_mem` size (`clouds.cc`).
const LARGE_BUFFER_SIZE: usize = 118784;
/// `block_ccm` size (`clouds.cc`): `65536 - 128`.
const SMALL_BUFFER_SIZE: usize = 65536 - 128;

/// `PlaybackMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackMode {
    /// `PLAYBACK_MODE_GRANULAR`.
    Granular,
    /// `PLAYBACK_MODE_STRETCH` -- WSOLA.
    Stretch,
    /// `PLAYBACK_MODE_LOOPING_DELAY`.
    LoopingDelay,
    /// `PLAYBACK_MODE_SPECTRAL` -- **not ported**; produces silence.
    Spectral,
}

/// `GranularProcessor`.
pub struct GranularProcessor {
    playback_mode: PlaybackMode,
    previous_playback_mode: Option<PlaybackMode>,
    num_channels: i32,
    low_fidelity: bool,

    silence: bool,
    bypass: bool,
    reset_buffers: bool,
    freeze_lp: f32,
    dry_wet: f32,

    correlator: Correlator,

    player: GranularSamplePlayer,
    ws_player: WSOLASamplePlayer,
    looper: LoopingSamplePlayer,

    diffuser: Diffuser,
    reverb: Reverb,
    pitch_shifter: PitchShifter,
    fb_filter: [Svf; 2],
    hp_filter: [Svf; 2],
    lp_filter: [Svf; 2],

    buffer_8: [AudioBuffer; 2],
    buffer_16: [AudioBuffer; 2],

    in_: Box<[FloatFrame]>,
    out_: Box<[FloatFrame]>,
    fb_: Box<[FloatFrame]>,

    parameters: Parameters,

    src_down: SampleRateConverter,
    src_up: SampleRateConverter,
}

impl GranularProcessor {
    /// `GranularProcessor()` + `Init` -- allocates the owned recording slabs
    /// and post-FX buffers.
    pub fn new() -> Self {
        use alloc::vec;
        let mut gp = Self {
            playback_mode: PlaybackMode::Granular,
            previous_playback_mode: None,
            num_channels: 2,
            low_fidelity: false,
            silence: false,
            bypass: false,
            reset_buffers: true,
            freeze_lp: 0.0,
            dry_wet: 0.0,
            correlator: Correlator::new(),
            player: GranularSamplePlayer::new(),
            ws_player: WSOLASamplePlayer::new(),
            looper: LoopingSamplePlayer::new(),
            diffuser: Diffuser::new(),
            reverb: Reverb::new(),
            pitch_shifter: PitchShifter::new(),
            fb_filter: [Svf::default(); 2],
            hp_filter: [Svf::default(); 2],
            lp_filter: [Svf::default(); 2],
            buffer_8: [AudioBuffer::new(), AudioBuffer::new()],
            buffer_16: [AudioBuffer::new(), AudioBuffer::new()],
            in_: vec![FloatFrame::default(); MAX_BLOCK_SIZE].into_boxed_slice(),
            out_: vec![FloatFrame::default(); MAX_BLOCK_SIZE].into_boxed_slice(),
            fb_: vec![FloatFrame::default(); MAX_BLOCK_SIZE].into_boxed_slice(),
            parameters: Parameters::default(),
            src_down: SampleRateConverter::new(-2, &SRC_FILTER_1X_2_45),
            src_up: SampleRateConverter::new(2, &SRC_FILTER_1X_2_45),
        };
        gp.reset_filters();
        gp
    }

    /// `mutable_parameters()`.
    #[inline]
    pub fn mutable_parameters(&mut self) -> &mut Parameters {
        &mut self.parameters
    }

    /// `parameters()`.
    #[inline]
    pub fn parameters(&self) -> &Parameters {
        &self.parameters
    }

    /// `ToggleFreeze`.
    #[inline]
    pub fn toggle_freeze(&mut self) {
        self.parameters.freeze = !self.parameters.freeze;
    }

    /// `set_freeze`.
    #[inline]
    pub fn set_freeze(&mut self, freeze: bool) {
        self.parameters.freeze = freeze;
    }

    /// `frozen`.
    #[inline]
    pub fn frozen(&self) -> bool {
        self.parameters.freeze
    }

    /// `set_silence`.
    #[inline]
    pub fn set_silence(&mut self, silence: bool) {
        self.silence = silence;
    }

    /// `set_bypass`.
    #[inline]
    pub fn set_bypass(&mut self, bypass: bool) {
        self.bypass = bypass;
    }

    /// `bypass`.
    #[inline]
    pub fn bypass(&self) -> bool {
        self.bypass
    }

    /// `set_playback_mode`.
    #[inline]
    pub fn set_playback_mode(&mut self, playback_mode: PlaybackMode) {
        self.playback_mode = playback_mode;
    }

    /// `playback_mode`.
    #[inline]
    pub fn playback_mode(&self) -> PlaybackMode {
        self.playback_mode
    }

    /// `set_quality` -- `bit 0` = mono, `bit 1` = low fidelity.
    #[inline]
    pub fn set_quality(&mut self, quality: i32) {
        self.set_num_channels(if quality & 1 != 0 { 1 } else { 2 });
        self.set_low_fidelity(quality >> 1 != 0);
    }

    /// `set_num_channels`.
    #[inline]
    pub fn set_num_channels(&mut self, num_channels: i32) {
        self.reset_buffers = self.reset_buffers || self.num_channels != num_channels;
        self.num_channels = num_channels;
    }

    /// `set_low_fidelity`.
    #[inline]
    pub fn set_low_fidelity(&mut self, low_fidelity: bool) {
        self.reset_buffers = self.reset_buffers || low_fidelity != self.low_fidelity;
        self.low_fidelity = low_fidelity;
    }

    /// `quality`.
    #[inline]
    pub fn quality(&self) -> i32 {
        let mut q = 0;
        if self.num_channels == 1 {
            q |= 1;
        }
        if self.low_fidelity {
            q |= 2;
        }
        q
    }

    #[inline]
    fn resolution(&self) -> Resolution {
        if self.low_fidelity {
            Resolution::Bit8MuLaw
        } else {
            Resolution::Bit16
        }
    }

    #[inline]
    fn sample_rate(&self) -> f32 {
        32000.0 / if self.low_fidelity { DOWNSAMPLING_FACTOR as f32 } else { 1.0 }
    }

    fn reset_filters(&mut self) {
        for i in 0..2 {
            self.fb_filter[i].init();
            self.lp_filter[i].init();
            self.hp_filter[i].init();
        }
    }

    /// `Prepare` -- run once per block *before* [`process`](Self::process);
    /// (re)allocates buffers on a quality/mode change and primes the WSOLA
    /// correlator.
    pub fn prepare(&mut self) {
        let playback_mode_changed = self.previous_playback_mode != Some(self.playback_mode);
        let benign_change = match self.previous_playback_mode {
            Some(prev) => {
                prev != PlaybackMode::Spectral && self.playback_mode != PlaybackMode::Spectral
            }
            None => false,
        };

        if !self.reset_buffers && playback_mode_changed && benign_change {
            self.reset_filters();
            self.pitch_shifter.clear();
            self.previous_playback_mode = Some(self.playback_mode);
        }

        if (playback_mode_changed && !benign_change) || self.reset_buffers {
            self.parameters.freeze = false;
        }

        if self.reset_buffers || (playback_mode_changed && !benign_change) {
            let (buffer_size_0, buffer_size_1) = if self.num_channels == 1 {
                (LARGE_BUFFER_SIZE, 0)
            } else {
                (SMALL_BUFFER_SIZE, SMALL_BUFFER_SIZE)
            };

            self.diffuser.init();
            self.reverb.init();
            self.correlator.init();
            self.pitch_shifter.init();

            if self.playback_mode == PlaybackMode::Spectral {
                // Phase-vocoder mode is not ported; leave the buffers idle.
            } else {
                let resolution = self.resolution();
                let sizes = [buffer_size_0, buffer_size_1];
                for i in 0..self.num_channels as usize {
                    match resolution {
                        Resolution::Bit8MuLaw => {
                            self.buffer_8[i].init(resolution, sizes[i] as i32);
                        }
                        Resolution::Bit16 => {
                            self.buffer_16[i].init(resolution, (sizes[i] >> 1) as i32);
                        }
                    }
                }
                let per_channel = if self.num_channels == 1 { 40 } else { 32 };
                let per_fidelity = if self.low_fidelity { 23 } else { 16 };
                let num_grains = (per_channel * per_fidelity) >> 4;
                self.player.init(self.num_channels, num_grains);
                self.ws_player.init(self.num_channels);
                self.looper.init(self.num_channels);
            }
            self.reset_buffers = false;
            self.previous_playback_mode = Some(self.playback_mode);
        }

        if self.playback_mode == PlaybackMode::Spectral {
            // phase_vocoder_.Buffer();
        } else if self.playback_mode == PlaybackMode::Stretch {
            let resolution = self.resolution();
            // Split the borrow: correlator + ws_player + one buffer array.
            match resolution {
                Resolution::Bit8MuLaw => {
                    self.ws_player.load_correlator(&mut self.correlator, &self.buffer_8);
                }
                Resolution::Bit16 => {
                    self.ws_player.load_correlator(&mut self.correlator, &self.buffer_16);
                }
            }
            self.correlator.evaluate_some_candidates();
        }
    }

    #[inline]
    fn active_buffer_mut(&mut self) -> &mut [AudioBuffer] {
        match self.resolution() {
            Resolution::Bit8MuLaw => &mut self.buffer_8,
            Resolution::Bit16 => &mut self.buffer_16,
        }
    }

    /// `ProcessGranular`.
    fn process_granular(&mut self, input: &[FloatFrame], output: &mut [FloatFrame], size: usize) {
        let resolution = self.resolution();
        // Every mode except spectral records the incoming audio.
        if self.playback_mode != PlaybackMode::Spectral {
            let write = !self.parameters.freeze;
            let num_channels = self.num_channels as usize;
            // Interleave the input so `write_fade`'s strided reads line up
            // with the C's `&input[0].l` pointer walk.
            let mut interleaved = [0.0f32; MAX_BLOCK_SIZE * 2];
            for (i, f) in input.iter().take(size).enumerate() {
                interleaved[2 * i] = f.l;
                interleaved[2 * i + 1] = f.r;
            }
            let buffers = self.active_buffer_mut();
            for (i, buffer) in buffers.iter_mut().take(num_channels).enumerate() {
                buffer.write_fade(&interleaved[i..], size, 2, write);
            }
        }

        let mut out_interleaved = [0.0f32; MAX_BLOCK_SIZE * 2];
        match self.playback_mode {
            PlaybackMode::Granular => {
                // DENSITY is a meta parameter in granular mode.
                self.parameters.granular.use_deterministic_seed = self.parameters.density < 0.5;
                let d = self.parameters.density;
                self.parameters.granular.overlap = if d >= 0.53 {
                    (d - 0.53) * 2.12
                } else if d <= 0.47 {
                    (0.47 - d) * 2.12
                } else {
                    0.0
                };
                self.parameters.granular.window_shape = if self.parameters.texture < 0.75 {
                    self.parameters.texture * 1.333
                } else {
                    1.0
                };
                let params = self.parameters;
                let buffers = match resolution {
                    Resolution::Bit8MuLaw => &self.buffer_8,
                    Resolution::Bit16 => &self.buffer_16,
                };
                self.player.play(buffers, &params, &mut out_interleaved, size);
            }
            PlaybackMode::Stretch => {
                let params = self.parameters;
                let buffers = match resolution {
                    Resolution::Bit8MuLaw => &self.buffer_8,
                    Resolution::Bit16 => &self.buffer_16,
                };
                self.ws_player.play(
                    &mut self.correlator,
                    buffers,
                    &params,
                    &mut out_interleaved,
                    size,
                );
            }
            PlaybackMode::LoopingDelay => {
                let params = self.parameters;
                let buffers = match resolution {
                    Resolution::Bit8MuLaw => &self.buffer_8,
                    Resolution::Bit16 => &self.buffer_16,
                };
                self.looper.play(buffers, &params, &mut out_interleaved, size);
            }
            PlaybackMode::Spectral => {
                // Not ported -- silence.
                out_interleaved.fill(0.0);
            }
        }

        for i in 0..size {
            output[i].l = out_interleaved[2 * i];
            output[i].r = out_interleaved[2 * i + 1];
        }
    }

    /// `Process` -- one block of at most [`MAX_BLOCK_SIZE`] stereo samples.
    pub fn process(&mut self, input: &[ShortFrame], output: &mut [ShortFrame]) {
        let size = input.len().min(output.len()).min(MAX_BLOCK_SIZE);

        if self.bypass {
            output[..size].copy_from_slice(&input[..size]);
            return;
        }

        if self.silence
            || self.reset_buffers
            || self.previous_playback_mode != Some(self.playback_mode)
        {
            for o in output[..size].iter_mut() {
                *o = ShortFrame::default();
            }
            return;
        }

        for i in 0..size {
            self.in_[i].l = input[i].l as f32 / 32768.0;
            self.in_[i].r = input[i].r as f32 / 32768.0;
        }
        if self.num_channels == 1 {
            for i in 0..size {
                self.in_[i].l = (self.in_[i].l + self.in_[i].r) * 0.5;
                self.in_[i].r = self.in_[i].l;
            }
        }

        // Feedback path with a high-pass to stop low-frequency build-ups.
        one_pole(
            &mut self.freeze_lp,
            if self.parameters.freeze { 1.0 } else { 0.0 },
            0.0005,
        );
        let feedback = self.parameters.feedback;
        let cutoff = (20.0 + 100.0 * feedback * feedback) / self.sample_rate();
        self.fb_filter[0].set_f_q(cutoff, 1.0, FrequencyApproximation::Fast);
        let fb0 = self.fb_filter[0];
        self.fb_filter[1].set(&fb0);
        for i in 0..size {
            self.fb_[i].l = self.fb_filter[0].process(FilterMode::HighPass, self.fb_[i].l);
        }
        for i in 0..size {
            self.fb_[i].r = self.fb_filter[1].process(FilterMode::HighPass, self.fb_[i].r);
        }
        let fb_gain = feedback * (1.0 - self.freeze_lp);
        for i in 0..size {
            let (inl, inr) = (self.in_[i].l, self.in_[i].r);
            self.in_[i].l += fb_gain * (soft_limit(fb_gain * 1.4 * self.fb_[i].l + inl) - inl);
            self.in_[i].r += fb_gain * (soft_limit(fb_gain * 1.4 * self.fb_[i].r + inr) - inr);
        }

        if self.low_fidelity {
            let downsampled_size = size / DOWNSAMPLING_FACTOR;
            let mut in_ds = [FloatFrame::default(); MAX_BLOCK_SIZE / DOWNSAMPLING_FACTOR];
            let mut out_ds = [FloatFrame::default(); MAX_BLOCK_SIZE / DOWNSAMPLING_FACTOR];
            let in_copy: [FloatFrame; MAX_BLOCK_SIZE] =
                core::array::from_fn(|i| if i < size { self.in_[i] } else { FloatFrame::default() });
            self.src_down.process(&in_copy, &mut in_ds, size);
            self.process_granular(&in_ds, &mut out_ds, downsampled_size);
            let mut out_full = [FloatFrame::default(); MAX_BLOCK_SIZE];
            self.src_up.process(&out_ds, &mut out_full, downsampled_size);
            self.out_[..size].copy_from_slice(&out_full[..size]);
        } else {
            let in_copy: [FloatFrame; MAX_BLOCK_SIZE] = core::array::from_fn(|i| {
                if i < size {
                    self.in_[i]
                } else {
                    FloatFrame::default()
                }
            });
            let mut out_scratch = [FloatFrame::default(); MAX_BLOCK_SIZE];
            self.process_granular(&in_copy, &mut out_scratch, size);
            self.out_[..size].copy_from_slice(&out_scratch[..size]);
        }

        // Diffusion.
        if self.playback_mode != PlaybackMode::Spectral {
            let texture = self.parameters.texture;
            let diffusion = if self.playback_mode == PlaybackMode::Granular {
                if texture > 0.75 {
                    (texture - 0.75) * 4.0
                } else {
                    0.0
                }
            } else {
                self.parameters.density
            };
            self.diffuser.set_amount(diffusion);
            self.diffuser.process(&mut self.out_[..size]);
        }

        // Pitch-shifting (looping delay only).
        if self.playback_mode == PlaybackMode::LoopingDelay
            && (!self.parameters.freeze || self.looper.synchronized())
        {
            self.pitch_shifter
                .set_ratio(semitones_to_ratio(self.parameters.pitch));
            self.pitch_shifter.set_size(self.parameters.size);
            self.pitch_shifter.process(&mut self.out_[..size]);
        }

        // Tone filters.
        if self.playback_mode == PlaybackMode::LoopingDelay
            || self.playback_mode == PlaybackMode::Stretch
        {
            let cutoff = self.parameters.texture;
            let mut lp_cutoff = 0.5
                * semitones_to_ratio((if cutoff < 0.5 { cutoff - 0.5 } else { 0.0 }) * 216.0);
            let mut hp_cutoff = 0.25
                * semitones_to_ratio((if cutoff < 0.5 { -0.5 } else { cutoff - 1.0 }) * 216.0);
            lp_cutoff = constrain(lp_cutoff, 0.0, 0.499);
            hp_cutoff = constrain(hp_cutoff, 0.0, 0.499);
            let lpq = 1.0 + 3.0 * (1.0 - feedback) * (0.5 - lp_cutoff);

            self.lp_filter[0].set_f_q(lp_cutoff, lpq, FrequencyApproximation::Fast);
            for i in 0..size {
                self.out_[i].l = self.lp_filter[0].process(FilterMode::LowPass, self.out_[i].l);
            }
            let lp0 = self.lp_filter[0];
            self.lp_filter[1].set(&lp0);
            for i in 0..size {
                self.out_[i].r = self.lp_filter[1].process(FilterMode::LowPass, self.out_[i].r);
            }

            self.hp_filter[0].set_f_q(hp_cutoff, 1.0, FrequencyApproximation::Fast);
            for i in 0..size {
                self.out_[i].l = self.hp_filter[0].process(FilterMode::HighPass, self.out_[i].l);
            }
            let hp0 = self.hp_filter[0];
            self.hp_filter[1].set(&hp0);
            for i in 0..size {
                self.out_[i].r = self.hp_filter[1].process(FilterMode::HighPass, self.out_[i].r);
            }
        }

        // This is what is fed back -- reverb is not.
        for i in 0..size {
            self.fb_[i] = self.out_[i];
        }

        // Reverb.
        let mut reverb_amount = self.parameters.reverb * 0.95;
        reverb_amount += feedback * (2.0 - feedback) * self.freeze_lp;
        reverb_amount = constrain(reverb_amount, 0.0, 1.0);
        self.reverb.set_amount(reverb_amount * 0.54);
        self.reverb.set_diffusion(0.7);
        self.reverb.set_time(0.35 + 0.63 * reverb_amount);
        self.reverb.set_input_gain(0.2);
        self.reverb.set_lp(0.6 + 0.37 * feedback);
        self.reverb.process(&mut self.out_[..size]);

        const POST_GAIN: f32 = 1.2;
        let mut dry_wet_mod = ParameterInterpolator::new(&mut self.dry_wet, self.parameters.dry_wet, size);
        for i in 0..size {
            let dry_wet = dry_wet_mod.next();
            let fade_in = interpolate(&LUT_XFADE_IN, dry_wet, 16.0);
            let fade_out = interpolate(&LUT_XFADE_OUT, dry_wet, 16.0);
            let mut l = input[i].l as f32 / 32768.0 * fade_out;
            let mut r = input[i].r as f32 / 32768.0 * fade_out;
            l += self.out_[i].l * POST_GAIN * fade_in;
            r += self.out_[i].r * POST_GAIN * fade_in;
            output[i].l = soft_convert(l);
            output[i].r = soft_convert(r);
        }
    }
}

impl Default for GranularProcessor {
    fn default() -> Self {
        Self::new()
    }
}
