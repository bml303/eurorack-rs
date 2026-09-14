//! `stages/segment_generator.{h,cc}` -- the heart of Stages: a per-channel
//! engine that can behave as a multi-segment envelope, an LFO (free-running,
//! tap-tempo, or hard-synced/PLL), a step sequencer, a sample & hold, a
//! portamento slide, a clocked delay, or an audio-rate oscillator, selected
//! by [`SegmentGenerator::configure`].
//!
//! # Pointer fields -> a `Field` enum
//!
//! The C's `Segment` struct holds `float*` members that alias one of three
//! per-instance constants (`zero_ = 0.0`, `half_ = 0.5`, `one_ = 1.0`, never
//! mutated after `Init`) or a field of `parameters_[i]`, so that several
//! segments can share backing storage and `Configure` can rewire a segment by
//! repointing a pointer instead of copying a value. That's exactly a "which
//! of a few known sources" enum with no need for unsafe raw pointers or
//! lifetimes; see [`Field`] and [`SegmentGenerator::read`]. A `None` (C
//! `NULL`) means something different per field: for `start`, "begin from the
//! current running value"; for `time`, "infinite duration (hold)"; for
//! `phase`, "use the real accumulating ramp phase" instead of a fixed 0.0/1.0
//! sample-or-track flag.

use crate::delay_line_16_bits::DelayLine16Bits;
use crate::resources::LUT_SINE;
use crate::variable_shape_oscillator::VariableShapeOscillator;
use stmlib::fdsp::{crossfade, interpolate_wrap, one_pole};
use stmlib::gate_flags::GateFlags;
use stmlib::hysteresis_quantizer::HysteresisQuantizer2;
use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::random::Random;
use stmlib::units::semitones_to_ratio;
use tides2::ramp_extractor::RampExtractor;
use tides2::ratio::Ratio;

use crate::resources::{LUT_ENV_FREQUENCY, LUT_PORTAMENTO_COEFFICIENT};

pub const K_SAMPLE_RATE: f32 = 31250.0;

/// Each segment generator can handle up to 36 segments. Matches the C's
/// `kMaxNumSegments` (a per-instance budget, not a shared pool).
pub const K_MAX_NUM_SEGMENTS: usize = 36;
pub const K_MAX_DELAY: usize = 576;

/// Largest block size this port's stack-allocated scratch buffers support
/// (the C's `ProcessOscillator` uses a variable-length-array `float
/// ramp[size]`). The firmware always calls in blocks of `kBlockSize == 8`
/// (`stages::kBlockSize`, in the out-of-scope `io_buffer.h`); this is well
/// above that for headroom.
pub const MAX_BLOCK_SIZE: usize = 64;

const K_RETRIG_DELAY_SAMPLES: i32 = 32;
/// `kSampleAndHoldDelay = kSampleRate * 2 / 1000` (31250.0 * 2 / 1000 = 62.5,
/// truncated to `size_t`).
const K_SAMPLE_AND_HOLD_DELAY: usize = 62;
/// `kClockInhibitDelay = kSampleRate * 5 / 1000` (31250.0 * 5 / 1000 = 156.25,
/// truncated to `size_t`).
const K_CLOCK_INHIBIT_DELAY: i32 = 156;

pub mod segment {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub enum Type {
        #[default]
        Ramp,
        Step,
        Hold,
        Alt,
    }

    #[derive(Debug, Clone, Copy, Default)]
    pub struct Configuration {
        pub kind: Type,
        pub looping: bool,
    }

    #[derive(Debug, Clone, Copy, Default)]
    pub struct Parameters {
        pub primary: f32,
        pub secondary: f32,
    }
}

use segment::{Configuration, Parameters, Type};

#[derive(Debug, Clone, Copy, Default)]
pub struct Output {
    pub value: f32,
    pub phase: f32,
    pub segment: i32,
}

/// One of the few backing-storage locations a `Segment`'s pointer-like
/// fields can alias. See the module doc comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Zero,
    Half,
    One,
    Primary(usize),
    Secondary(usize),
}

#[derive(Debug, Clone, Copy)]
struct Segment {
    start: Option<Field>,
    time: Option<Field>,
    curve: Field,
    portamento: Field,
    end: Field,
    phase: Option<Field>,

    if_rising: i32,
    if_falling: i32,
    if_complete: i32,
}

impl Default for Segment {
    fn default() -> Self {
        Self {
            start: Some(Field::Zero),
            time: Some(Field::Zero),
            curve: Field::Half,
            portamento: Field::Zero,
            end: Field::Zero,
            phase: None,
            if_rising: 0,
            if_falling: 0,
            if_complete: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Up,
    Down,
    UpDown,
    Alternating,
    Random,
    RandomWithoutRepeat,
    Addressable,
    Last,
}

fn direction_from_i32(v: i32) -> Direction {
    match v {
        0 => Direction::Up,
        1 => Direction::Down,
        2 => Direction::UpDown,
        3 => Direction::Alternating,
        4 => Direction::Random,
        5 => Direction::RandomWithoutRepeat,
        6 => Direction::Addressable,
        _ => Direction::Last,
    }
}

/// The C's `process_fn_table_[16]` function-pointer table, as a `match`
/// target instead. Slot 9 (`(has_trigger=false, loop=true, type=HOLD)`) is
/// `ProcessDelay`, *not* `ProcessClockedSampleAndHold` -- the C's table entry
/// for it is commented out ("`// &SegmentGenerator::ProcessClockedSampleAndHold,`"),
/// so [`SegmentGenerator::process_clocked_sample_and_hold`] is real,
/// translated, unreachable dead code in the shipped firmware too. Preserved
/// verbatim rather than "fixed", per this port's fidelity contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessFn {
    MultiSegment,
    Sequencer,
    DecayEnvelope,
    TimedPulseGenerator,
    GateGenerator,
    SampleAndHold,
    TapLfo,
    FreeRunningLfo,
    PllOscillator,
    FreeRunningOscillator,
    Delay,
    Portamento,
    Zero,
    #[allow(dead_code)]
    ClockedSampleAndHold,
    Slave,
}

#[rustfmt::skip]
const PROCESS_FN_TABLE: [ProcessFn; 16] = [
    // RAMP
    ProcessFn::Zero, ProcessFn::FreeRunningLfo, ProcessFn::DecayEnvelope, ProcessFn::TapLfo,
    // STEP
    ProcessFn::Portamento, ProcessFn::Portamento, ProcessFn::SampleAndHold, ProcessFn::SampleAndHold,
    // HOLD
    ProcessFn::Delay, ProcessFn::Delay, ProcessFn::TimedPulseGenerator, ProcessFn::GateGenerator,
    // ALT
    ProcessFn::Zero, ProcessFn::FreeRunningOscillator, ProcessFn::DecayEnvelope, ProcessFn::PllOscillator,
];

const DIVIDER_RATIOS: [Ratio; 7] = [
    Ratio { ratio: 0.249999, q: 4 },
    Ratio { ratio: 0.333333, q: 3 },
    Ratio { ratio: 0.499999, q: 2 },
    Ratio { ratio: 0.999999, q: 1 },
    Ratio { ratio: 1.999999, q: 1 },
    Ratio { ratio: 2.999999, q: 1 },
    Ratio { ratio: 3.999999, q: 1 },
];

/// The gate-delay ring buffer `mi-stmlib::DelayLine<T, N>` doesn't cover
/// (that one is `f32`-only; see its doc comment). Only an integer-delay
/// `read` is needed here (the C calls `Read(size_t)`, never the fractional
/// overload, for `GateFlags`).
#[derive(Debug, Clone)]
struct GateDelayLine<const N: usize> {
    line: [GateFlags; N],
    write_ptr: usize,
}

impl<const N: usize> Default for GateDelayLine<N> {
    fn default() -> Self {
        Self {
            line: [GateFlags::LOW; N],
            write_ptr: 0,
        }
    }
}

impl<const N: usize> GateDelayLine<N> {
    fn init(&mut self) {
        self.line = [GateFlags::LOW; N];
        self.write_ptr = 0;
    }

    #[inline]
    fn write(&mut self, sample: GateFlags) {
        self.line[self.write_ptr] = sample;
        self.write_ptr = (self.write_ptr + N - 1) % N;
    }

    #[inline]
    fn read(&self, delay: usize) -> GateFlags {
        self.line[(self.write_ptr + delay) % N]
    }
}

#[inline]
fn warp_phase(t: f32, curve: f32) -> f32 {
    let curve = curve - 0.5;
    let flip = curve < 0.0;
    let mut t = if flip { 1.0 - t } else { t };
    let a = 128.0 * curve * curve;
    t = (1.0 + a) * t / (1.0 + a * t);
    if flip {
        1.0 - t
    } else {
        t
    }
}

/// `RateToFrequency`. The C's `CONSTRAIN(i, 0, LUT_ENV_FREQUENCY_SIZE)`
/// clamps the index to `[0, 4096]` inclusive against a 4096-entry table (the
/// table itself has no extra guard sample here, unlike most of this
/// codebase's LUTs) -- an off-by-one that reads one past the end of
/// `lut_env_frequency` whenever `rate >= 2.0`. Clamped to the last valid
/// index instead; see this crate's `PORTING.md`.
#[inline]
fn rate_to_frequency(rate: f32) -> f32 {
    let i = (rate * 2048.0) as i32;
    let i = i.clamp(0, LUT_ENV_FREQUENCY.len() as i32 - 1);
    LUT_ENV_FREQUENCY[i as usize]
}

/// `PortamentoRateToLPCoefficient`. Unlike `RateToFrequency`, the C applies
/// *no* bounds check at all here (`lut_portamento_coefficient[i]` with a
/// completely unclamped `i`); clamped to the table's valid range instead.
/// See this crate's `PORTING.md`.
#[inline]
fn portamento_rate_to_lp_coefficient(rate: f32) -> f32 {
    let i = (rate * 512.0) as i32;
    let i = i.clamp(0, LUT_PORTAMENTO_COEFFICIENT.len() as i32 - 1);
    LUT_PORTAMENTO_COEFFICIENT[i as usize]
}

#[inline]
fn log2_fast(x: f32) -> f32 {
    let mut w = x.to_bits() as i32;
    let mut log2f = (((w >> 23) & 255) - 128) as f32;
    w &= !(255 << 23);
    w += 127 << 23;
    let r_f = f32::from_bits(w as u32);
    log2f += ((-0.34484843) * r_f + 2.024_665_8) * r_f - 0.674_877_6;
    log2f
}

fn shape_lfo(shape: f32, input_phase: &[f32], out: &mut [Output]) {
    let shape = shape - 0.5;
    let shape = 2.0 + 9.999999 * shape / (1.0 + 3.0 * shape.abs());

    let slope = (shape * 0.5).min(0.5);
    let plateau_width = (shape - 3.0).max(0.0);
    let sine_amount = (if shape < 2.0 { shape - 1.0 } else { 3.0 - shape }).max(0.0);

    let slope_up = 1.0 / slope;
    let slope_down = 1.0 / (1.0 - slope);
    let plateau = 0.5 * (1.0 - plateau_width);
    let normalization = 1.0 / plateau;
    let phase_shift = plateau_width * 0.25;

    for (i, o) in out.iter_mut().enumerate() {
        let mut phase = input_phase[i] + phase_shift;
        if phase > 1.0 {
            phase -= 1.0;
        }
        let mut triangle = if phase < slope {
            slope_up * phase
        } else {
            1.0 - (phase - slope) * slope_down
        };
        triangle -= 0.5;
        triangle = triangle.clamp(-plateau, plateau);
        triangle *= normalization;
        let sine = interpolate_wrap(&LUT_SINE, phase + 0.75, 1024.0);
        o.phase = input_phase[i];
        o.value = 0.5 * crossfade(triangle, sine, sine_amount) + 0.5;
        o.segment = if phase < 0.5 { 0 } else { 1 };
    }
}

pub struct SegmentGenerator {
    phase: f32,
    aux: f32,

    start: f32,
    value: f32,
    lp: f32,
    primary: f32,

    active_segment: i32,
    previous_segment: i32,
    monitored_segment: i32,
    retrig_delay: i32,

    num_segments: i32,

    process_fn: ProcessFn,

    ramp_extractor: RampExtractor,
    function_quantizer: HysteresisQuantizer2,

    segments: [Segment; K_MAX_NUM_SEGMENTS + 1],
    parameters: [Parameters; K_MAX_NUM_SEGMENTS],

    delay_line: DelayLine16Bits<K_MAX_DELAY>,
    gate_delay: GateDelayLine<128>,

    first_step: i32,
    last_step: i32,
    quantized_output: bool,

    up_down_counter: i32,
    reset: bool,
    accepted_gate: bool,
    inhibit_clock: i32,
    address_quantizer: HysteresisQuantizer2,
    /// The C's `step_quantizer_` is an externally-owned `HysteresisQuantizer2*`
    /// -- in the firmware, `stages.cc` hands every channel a pointer into one
    /// shared `note_quantizer[kNumChannels + kMaxNumSegments]` array, at an
    /// offset equal to the channel index, so neighbouring channels'
    /// quantizer windows actually overlap (`channel i` owns
    /// `note_quantizer[i .. i + active_segment]`). That sharing is
    /// `stages.cc` wiring (out of scope, see `PORTING.md`), not a documented
    /// DSP contract, so this port gives each `SegmentGenerator` its own
    /// owned bank instead of a borrowed external pointer -- same per-channel
    /// behaviour, minus the cross-channel aliasing quirk.
    step_quantizer: [HysteresisQuantizer2; K_MAX_NUM_SEGMENTS],
    has_step_quantizer: bool,

    audio_osc: VariableShapeOscillator,
}

impl Default for SegmentGenerator {
    fn default() -> Self {
        Self {
            phase: 0.0,
            aux: 0.0,
            start: 0.0,
            value: 0.0,
            lp: 0.0,
            primary: 0.0,
            active_segment: 0,
            previous_segment: 0,
            monitored_segment: 0,
            retrig_delay: 0,
            num_segments: 0,
            process_fn: ProcessFn::MultiSegment,
            ramp_extractor: RampExtractor::new(),
            function_quantizer: HysteresisQuantizer2::default(),
            segments: [Segment::default(); K_MAX_NUM_SEGMENTS + 1],
            parameters: [Parameters::default(); K_MAX_NUM_SEGMENTS],
            delay_line: DelayLine16Bits::default(),
            gate_delay: GateDelayLine::default(),
            first_step: 1,
            last_step: 1,
            quantized_output: false,
            up_down_counter: 0,
            reset: false,
            accepted_gate: true,
            inhibit_clock: 0,
            address_quantizer: HysteresisQuantizer2::default(),
            step_quantizer: [HysteresisQuantizer2::default(); K_MAX_NUM_SEGMENTS],
            has_step_quantizer: false,
            audio_osc: VariableShapeOscillator::default(),
        }
    }
}

impl SegmentGenerator {
    pub fn new() -> Self {
        Self::default()
    }

    /// `Init()` / `Init(HysteresisQuantizer2* step_quantizer)`. Pass `true`
    /// for `quantized_step_output` where the C would pass a non-null
    /// `step_quantizer` (see the field's doc comment for how ownership
    /// differs).
    pub fn init(&mut self, quantized_step_output: bool) {
        self.process_fn = ProcessFn::MultiSegment;

        self.phase = 0.0;

        self.start = 0.0;
        self.value = 0.0;
        self.lp = 0.0;

        self.monitored_segment = 0;
        self.active_segment = 0;
        self.previous_segment = 0;
        self.retrig_delay = 0;
        self.primary = 0.0;

        self.segments = [Segment::default(); K_MAX_NUM_SEGMENTS + 1];
        self.parameters = [Parameters::default(); K_MAX_NUM_SEGMENTS];

        self.ramp_extractor.init(K_SAMPLE_RATE, 1000.0 / K_SAMPLE_RATE);

        self.delay_line.init();
        self.gate_delay.init();

        self.function_quantizer.init(2, 0.025, false);
        self.address_quantizer.init(2, 0.025, false);

        self.num_segments = 0;

        self.first_step = 1;
        self.last_step = 1;
        self.quantized_output = false;
        self.up_down_counter = 0;
        self.inhibit_clock = 0;
        self.reset = false;
        self.accepted_gate = true;

        self.has_step_quantizer = quantized_step_output;
        if quantized_step_output {
            for q in self.step_quantizer.iter_mut() {
                q.init(13, 0.03, false);
            }
        }

        self.audio_osc.init();
    }

    #[inline]
    fn read(&self, f: Field) -> f32 {
        match f {
            Field::Zero => 0.0,
            Field::Half => 0.5,
            Field::One => 1.0,
            Field::Primary(i) => self.parameters[i].primary,
            Field::Secondary(i) => self.parameters[i].secondary,
        }
    }

    #[inline]
    pub fn num_segments(&self) -> i32 {
        self.num_segments
    }

    pub fn set_segment_parameters(&mut self, index: usize, primary: f32, secondary: f32) {
        self.parameters[index].primary = primary;
        self.parameters[index].secondary = secondary;
    }

    pub fn configure_single_segment(&mut self, has_trigger: bool, segment_configuration: Configuration) {
        let mut i = if has_trigger { 2 } else { 0 };
        i += if segment_configuration.looping { 1 } else { 0 };
        i += segment_configuration.kind as usize * 4;
        self.process_fn = PROCESS_FN_TABLE[i];
        self.num_segments = 1;
    }

    pub fn configure_slave(&mut self, monitored_segment: i32) {
        self.monitored_segment = monitored_segment;
        self.process_fn = ProcessFn::Slave;
        self.num_segments = 0;
    }

    /// Returns `true` when `active_segment == 0` after processing (mirrors
    /// the C's `Process`, which returns that as a "trigger accepted /
    /// segment 0 active" flag).
    pub fn process(&mut self, gate_flags: &[GateFlags], out: &mut [Output]) -> bool {
        match self.process_fn {
            ProcessFn::MultiSegment => self.process_multi_segment(gate_flags, out),
            ProcessFn::Sequencer => self.process_sequencer(gate_flags, out),
            ProcessFn::DecayEnvelope => self.process_decay_envelope(gate_flags, out),
            ProcessFn::TimedPulseGenerator => self.process_timed_pulse_generator(gate_flags, out),
            ProcessFn::GateGenerator => self.process_gate_generator(gate_flags, out),
            ProcessFn::SampleAndHold => self.process_sample_and_hold(gate_flags, out),
            ProcessFn::TapLfo => self.process_tap_lfo(gate_flags, out),
            ProcessFn::FreeRunningLfo => self.process_free_running_lfo(gate_flags, out),
            ProcessFn::PllOscillator => self.process_pll_oscillator(gate_flags, out),
            ProcessFn::FreeRunningOscillator => self.process_free_running_oscillator(gate_flags, out),
            ProcessFn::Delay => self.process_delay(gate_flags, out),
            ProcessFn::Portamento => self.process_portamento(gate_flags, out),
            ProcessFn::Zero => self.process_zero(gate_flags, out),
            ProcessFn::ClockedSampleAndHold => self.process_clocked_sample_and_hold(out),
            ProcessFn::Slave => self.process_slave(out),
        }
        self.active_segment == 0
    }

    fn process_multi_segment(&mut self, gate_flags: &[GateFlags], out: &mut [Output]) {
        let size = out.len().min(gate_flags.len());
        let mut phase = self.phase;
        let mut start = self.start;
        let mut lp = self.lp;
        let mut value = self.value;

        for i in 0..size {
            let segment = self.segments[self.active_segment as usize];
            let previous = self.segments[self.previous_segment as usize];
            if segment.start.is_none() && previous.phase.is_some() && segment.end != previous.end {
                one_pole(
                    &mut start,
                    self.read(previous.end),
                    portamento_rate_to_lp_coefficient(self.read(previous.portamento)),
                );
            }

            if let Some(t) = segment.time {
                phase += rate_to_frequency(self.read(t));
            }

            let complete = phase >= 1.0;
            if complete {
                phase = 1.0;
            }
            let warp_input = match segment.phase {
                Some(f) => self.read(f),
                None => phase,
            };
            value = crossfade(start, self.read(segment.end), warp_phase(warp_input, self.read(segment.curve)));

            one_pole(&mut lp, value, portamento_rate_to_lp_coefficient(self.read(segment.portamento)));

            let mut go_to_segment: i32 = -1;
            if gate_flags[i].contains(GateFlags::RISING) {
                go_to_segment = segment.if_rising;
            } else if gate_flags[i].contains(GateFlags::FALLING) {
                go_to_segment = segment.if_falling;
            } else if complete {
                go_to_segment = segment.if_complete;
            }

            if go_to_segment != -1 {
                phase = 0.0;
                let destination = self.segments[go_to_segment as usize];
                start = match destination.start {
                    Some(f) => self.read(f),
                    None => {
                        if go_to_segment == self.active_segment {
                            start
                        } else {
                            value
                        }
                    }
                };
                if go_to_segment != self.active_segment {
                    self.previous_segment = self.active_segment;
                }
                self.active_segment = go_to_segment;
            }

            out[i].value = lp;
            out[i].phase = phase;
            out[i].segment = self.active_segment;
        }
        self.phase = phase;
        self.start = start;
        self.lp = lp;
        self.value = value;
    }

    fn process_decay_envelope(&mut self, gate_flags: &[GateFlags], out: &mut [Output]) {
        let size = out.len().min(gate_flags.len());
        let frequency = rate_to_frequency(self.parameters[0].primary);
        for i in 0..size {
            if gate_flags[i].contains(GateFlags::RISING) {
                self.phase = 0.0;
                self.active_segment = 0;
            }

            self.phase += frequency;
            if self.phase >= 1.0 {
                self.phase = 1.0;
                self.active_segment = 1;
            }
            self.lp = 1.0 - warp_phase(self.phase, self.parameters[0].secondary);
            self.value = self.lp;
            out[i].value = self.lp;
            out[i].phase = self.phase;
            out[i].segment = self.active_segment;
        }
    }

    fn process_timed_pulse_generator(&mut self, gate_flags: &[GateFlags], out: &mut [Output]) {
        let size = out.len().min(gate_flags.len());
        let frequency = rate_to_frequency(self.parameters[0].secondary);

        let mut primary = ParameterInterpolator::new(&mut self.primary, self.parameters[0].primary, size);
        for i in 0..size {
            if gate_flags[i].contains(GateFlags::RISING) {
                self.retrig_delay = if self.active_segment == 0 { K_RETRIG_DELAY_SAMPLES } else { 0 };
                self.phase = 0.0;
                self.active_segment = 0;
            }
            if self.retrig_delay != 0 {
                self.retrig_delay -= 1;
            }
            self.phase += frequency;
            if self.phase >= 1.0 {
                self.phase = 1.0;
                self.active_segment = 1;
            }

            let p = primary.next();
            let v = if self.active_segment == 0 && self.retrig_delay == 0 { p } else { 0.0 };
            self.lp = v;
            self.value = v;
            out[i].value = v;
            out[i].phase = self.phase;
            out[i].segment = self.active_segment;
        }
    }

    fn process_gate_generator(&mut self, gate_flags: &[GateFlags], out: &mut [Output]) {
        let size = out.len().min(gate_flags.len());
        let mut primary = ParameterInterpolator::new(&mut self.primary, self.parameters[0].primary, size);
        for i in 0..size {
            if gate_flags[i].contains(GateFlags::RISING) {
                self.accepted_gate = Random::get_float() < self.parameters[0].secondary * 1.01;
            }
            self.active_segment = if gate_flags[i].contains(GateFlags::HIGH) && self.accepted_gate { 0 } else { 1 };

            let p = primary.next();
            let v = if self.active_segment == 0 { p } else { 0.0 };
            self.lp = v;
            self.value = v;
            out[i].value = v;
            out[i].phase = 0.5;
            out[i].segment = self.active_segment;
        }
    }

    fn process_sample_and_hold(&mut self, gate_flags: &[GateFlags], out: &mut [Output]) {
        let size = out.len().min(gate_flags.len());
        let coefficient = portamento_rate_to_lp_coefficient(self.parameters[0].secondary);
        let mut primary = ParameterInterpolator::new(&mut self.primary, self.parameters[0].primary, size);

        for i in 0..size {
            let p = primary.next();
            self.gate_delay.write(gate_flags[i]);
            if self.gate_delay.read(K_SAMPLE_AND_HOLD_DELAY).contains(GateFlags::RISING) {
                self.value = p;
            }
            self.active_segment = if gate_flags[i].contains(GateFlags::HIGH) { 0 } else { 1 };

            one_pole(&mut self.lp, self.value, coefficient);
            out[i].value = self.lp;
            out[i].phase = 0.5;
            out[i].segment = self.active_segment;
        }
    }

    /// `ProcessClockedSampleAndHold`. Ported for completeness, but -- like
    /// the C -- unreachable: `PROCESS_FN_TABLE`'s slot for it dispatches to
    /// [`Self::process_delay`] instead. See [`ProcessFn`]'s doc comment.
    #[allow(dead_code)]
    fn process_clocked_sample_and_hold(&mut self, out: &mut [Output]) {
        let size = out.len();
        let frequency = rate_to_frequency(self.parameters[0].secondary);
        let mut primary = ParameterInterpolator::new(&mut self.primary, self.parameters[0].primary, size);
        for o in out.iter_mut() {
            self.phase += frequency;
            if self.phase >= 1.0 {
                self.phase -= 1.0;

                let reset_time = self.phase / frequency;
                self.value = primary.subsample(1.0 - reset_time);
            }
            primary.next();
            self.active_segment = if self.phase < 0.5 { 0 } else { 1 };
            o.value = self.value;
            o.phase = self.phase;
            o.segment = self.active_segment;
        }
    }

    fn process_tap_lfo(&mut self, gate_flags: &[GateFlags], out: &mut [Output]) {
        self.process_oscillator(false, Some(gate_flags), out);
    }

    fn process_free_running_lfo(&mut self, _gate_flags: &[GateFlags], out: &mut [Output]) {
        self.process_oscillator(false, None, out);
    }

    fn process_pll_oscillator(&mut self, gate_flags: &[GateFlags], out: &mut [Output]) {
        self.process_oscillator(true, Some(gate_flags), out);
    }

    fn process_free_running_oscillator(&mut self, _gate_flags: &[GateFlags], out: &mut [Output]) {
        self.process_oscillator(true, None, out);
    }

    fn process_oscillator(&mut self, audio_rate: bool, gate_flags: Option<&[GateFlags]>, out: &mut [Output]) {
        let size = out.len();
        assert!(size <= MAX_BLOCK_SIZE, "SegmentGenerator::process called with too large a block");

        let root_note = if audio_rate { 261.625_55 } else { 2.0439497 };
        let mut ramp_buf = [0.0f32; MAX_BLOCK_SIZE];
        let ramp = &mut ramp_buf[..size];

        let mut r = Ratio { ratio: 1.0, q: 1 };
        let frequency;
        if let Some(gate_flags) = gate_flags {
            let idx = self.function_quantizer.process(self.parameters[0].primary * 1.03) as usize;
            r = DIVIDER_RATIOS[idx];
            frequency = self.ramp_extractor.process(audio_rate, false, r, gate_flags, ramp, size);
        } else {
            let f = (96.0 * (self.parameters[0].primary - 0.5)).clamp(-128.0, 127.0);
            frequency = semitones_to_ratio(f) * root_note / K_SAMPLE_RATE;
        }

        if audio_rate {
            self.audio_osc.render_macro(frequency, self.parameters[0].secondary, ramp);

            let distance_to_c = if frequency <= 0.0 {
                0.5
            } else {
                log2_fast(frequency / r.ratio * K_SAMPLE_RATE / root_note)
            };

            let distance_to_c_integral = distance_to_c as i32;
            let mut distance_to_c_fractional = distance_to_c - distance_to_c_integral as f32;
            if distance_to_c_fractional < -0.5 {
                distance_to_c_fractional += 1.0;
            } else if distance_to_c_fractional > 0.5 {
                distance_to_c_fractional -= 1.0;
            }
            let d = (2.0 * distance_to_c_fractional.abs()).min(1.0);

            let blink_frequency = size as f32 * (16.0 * d * (2.0 - d) + 0.125) / K_SAMPLE_RATE;
            self.phase += blink_frequency;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
            for i in 0..size {
                out[i].phase = ramp[i] * 2.0 - 1.0;
                out[i].value = ramp[i] * 5.0 / 8.0;
                out[i].segment = if self.phase < 0.5 { 0 } else { 1 };
            }
        } else {
            if gate_flags.is_none() {
                for r in ramp.iter_mut() {
                    self.phase += frequency;
                    if self.phase >= 1.0 {
                        self.phase -= 1.0;
                    }
                    *r = self.phase;
                }
            }
            shape_lfo(self.parameters[0].secondary, ramp, out);
        }
        self.active_segment = out[size - 1].segment;
    }

    fn process_delay(&mut self, _gate_flags: &[GateFlags], out: &mut [Output]) {
        let max_delay = (K_MAX_DELAY - 1) as f32;

        let mut delay_time = semitones_to_ratio(2.0 * (self.parameters[0].secondary - 0.5) * 36.0) * 0.5 * K_SAMPLE_RATE;
        let mut clock_frequency = 1.0;
        let delay_frequency = 1.0 / delay_time;

        if delay_time >= max_delay {
            clock_frequency = max_delay * delay_frequency;
            delay_time = max_delay;
        }
        let size = out.len();
        let mut primary = ParameterInterpolator::new(&mut self.primary, self.parameters[0].primary, size);

        self.active_segment = 0;
        for o in out.iter_mut() {
            self.phase += clock_frequency;
            one_pole(&mut self.lp, primary.next(), clock_frequency);
            if self.phase >= 1.0 {
                self.phase -= 1.0;
                self.delay_line.write(self.lp);
            }

            self.aux += delay_frequency;
            if self.aux >= 1.0 {
                self.aux -= 1.0;
            }
            self.active_segment = if self.aux < 0.5 { 0 } else { 1 };

            one_pole(&mut self.value, self.delay_line.read(delay_time - self.phase), clock_frequency);
            o.value = self.value;
            o.phase = self.aux;
            o.segment = self.active_segment;
        }
    }

    fn process_portamento(&mut self, _gate_flags: &[GateFlags], out: &mut [Output]) {
        let size = out.len();
        let coefficient = portamento_rate_to_lp_coefficient(self.parameters[0].secondary);
        let mut primary = ParameterInterpolator::new(&mut self.primary, self.parameters[0].primary, size);

        self.active_segment = 0;
        for o in out.iter_mut() {
            self.value = primary.next();
            one_pole(&mut self.lp, self.value, coefficient);
            o.value = self.lp;
            o.phase = 0.5;
            o.segment = self.active_segment;
        }
    }

    fn process_zero(&mut self, _gate_flags: &[GateFlags], out: &mut [Output]) {
        self.value = 0.0;
        self.active_segment = 1;
        for o in out.iter_mut() {
            o.value = 0.0;
            o.phase = 0.5;
            o.segment = 1;
        }
    }

    /// `ProcessSlave`. Reads `out[i].segment`/`.phase` as *input* -- the
    /// caller must have already populated `out` by processing the monitored
    /// generator (the one `configure_slave`'s `monitored_segment` refers to)
    /// into the same buffer; this only overwrites `.value` in place. That
    /// cross-generator wiring is done by `chain_state.cc` in the firmware
    /// (out of scope).
    fn process_slave(&mut self, out: &mut [Output]) {
        for o in out.iter_mut() {
            self.active_segment = if o.segment == self.monitored_segment { 0 } else { 1 };
            o.value = if self.active_segment != 0 { 0.0 } else { 1.0 - o.phase };
        }
    }

    fn process_sequencer(&mut self, gate_flags: &[GateFlags], out: &mut [Output]) {
        let size = out.len().min(gate_flags.len());
        let direction = direction_from_i32(self.function_quantizer.process(self.parameters[0].secondary));

        if direction == Direction::Addressable {
            self.reset = false;
            self.active_segment = self.address_quantizer.process(self.parameters[0].primary) + self.first_step;
        } else {
            if self.parameters[0].primary > 0.125 && !self.reset {
                self.reset = true;
                self.active_segment = if direction == Direction::Down { self.last_step } else { self.first_step };
                self.up_down_counter = 0;
                self.inhibit_clock = K_CLOCK_INHIBIT_DELAY;
            }
            if self.reset && self.parameters[0].primary < 0.0625 {
                self.reset = false;
            }
        }

        for i in 0..size {
            if self.inhibit_clock != 0 {
                self.inhibit_clock -= 1;
            }

            let clockable = self.inhibit_clock == 0 && !self.reset && direction != Direction::Addressable;

            if gate_flags[i].contains(GateFlags::RISING) && clockable {
                match direction {
                    Direction::Up => {
                        self.active_segment += 1;
                        if self.active_segment > self.last_step {
                            self.active_segment = self.first_step;
                        }
                    }
                    Direction::Down => {
                        self.active_segment -= 1;
                        if self.active_segment < self.first_step {
                            self.active_segment = self.last_step;
                        }
                    }
                    Direction::UpDown => {
                        let n = self.last_step - self.first_step + 1;
                        if n == 1 {
                            self.active_segment = self.first_step;
                        } else {
                            self.up_down_counter = (self.up_down_counter + 1) % (2 * (n - 1));
                            self.active_segment = self.first_step
                                + if self.up_down_counter < n {
                                    self.up_down_counter
                                } else {
                                    2 * (n - 1) - self.up_down_counter
                                };
                        }
                    }
                    Direction::Alternating => {
                        let n = self.last_step - self.first_step + 1;
                        if n == 1 {
                            self.active_segment = self.first_step;
                        } else if n == 2 {
                            self.up_down_counter = (self.up_down_counter + 1) % 2;
                            self.active_segment = self.first_step + self.up_down_counter;
                        } else {
                            self.up_down_counter = (self.up_down_counter + 1) % (4 * n - 8);
                            let idx = (self.up_down_counter - 1) / 2;
                            self.active_segment = self.first_step
                                + if self.up_down_counter & 1 != 0 {
                                    1 + if idx < (n - 1) { idx } else { 2 * (n - 2) - idx }
                                } else {
                                    0
                                };
                        }
                    }
                    Direction::Random => {
                        self.active_segment =
                            self.first_step + (Random::get_float() * (self.last_step - self.first_step + 1) as f32) as i32;
                    }
                    Direction::RandomWithoutRepeat => {
                        let n = self.last_step - self.first_step + 1;
                        let r = (Random::get_float() * (n - 1) as f32) as i32;
                        self.active_segment = self.first_step + ((self.active_segment - self.first_step + r + 1) % n);
                    }
                    Direction::Addressable | Direction::Last => {}
                }
            }

            self.value = self.parameters[self.active_segment as usize].primary;
            if self.quantized_output {
                let note = self.step_quantizer[self.active_segment as usize].process(self.value);
                self.value = note as f32 / 96.0;
            }

            one_pole(
                &mut self.lp,
                self.value,
                portamento_rate_to_lp_coefficient(self.parameters[self.active_segment as usize].secondary),
            );

            out[i].value = self.lp;
            out[i].phase = 0.0;
            out[i].segment = self.active_segment;
        }
    }

    pub fn configure_sequencer(&mut self, segment_configuration: &[Configuration], num_segments: usize) {
        self.num_segments = num_segments as i32;

        self.first_step = 0;
        for (i, cfg) in segment_configuration.iter().enumerate().take(num_segments).skip(1) {
            if cfg.looping {
                if self.first_step == 0 {
                    self.first_step = i as i32;
                    self.last_step = i as i32;
                } else {
                    self.last_step = i as i32;
                }
            }
        }
        if self.first_step == 0 {
            self.first_step = 1;
            self.last_step = num_segments as i32 - 1;
        }

        let num_steps = self.last_step - self.first_step + 1;
        self.address_quantizer.init(num_steps, 0.02 / 8.0 * num_steps as f32, false);

        self.inhibit_clock = 0;
        self.up_down_counter = 0;
        self.quantized_output = segment_configuration[0].kind == Type::Ramp && self.has_step_quantizer;
        self.reset = false;
        self.lp = 0.0;
        self.value = 0.0;
        self.active_segment = self.first_step;
        self.process_fn = ProcessFn::Sequencer;
    }

    pub fn configure(&mut self, has_trigger: bool, segment_configuration: &[Configuration], num_segments: usize) {
        if num_segments == 1 {
            self.function_quantizer.init(7, 0.025, false);
            self.configure_single_segment(has_trigger, segment_configuration[0]);
            return;
        }

        let mut sequencer_mode =
            segment_configuration[0].kind != Type::Step && !segment_configuration[0].looping && num_segments >= 3;
        for cfg in segment_configuration.iter().take(num_segments).skip(1) {
            sequencer_mode = sequencer_mode && cfg.kind == Type::Step;
        }
        if sequencer_mode {
            self.function_quantizer.init(Direction::Last as i32, 0.025, false);
            self.configure_sequencer(segment_configuration, num_segments);
            return;
        }

        self.num_segments = num_segments as i32;

        self.process_fn = ProcessFn::MultiSegment;

        let mut loop_start: i32 = -1;
        let mut loop_end: i32 = -1;
        let mut has_step_segments = false;
        let last_segment = num_segments - 1;
        let mut first_ramp_segment: i32 = -1;

        #[allow(clippy::needless_range_loop)]
        for i in 0..=last_segment {
            has_step_segments = has_step_segments || segment_configuration[i].kind == Type::Step;
            if segment_configuration[i].looping {
                if loop_start == -1 {
                    loop_start = i as i32;
                }
                loop_end = i as i32;
            }
            if segment_configuration[i].kind == Type::Ramp && first_ramp_segment == -1 {
                first_ramp_segment = i as i32;
            }
        }

        let mut has_step_segments_inside_loop = false;
        if loop_start != -1 {
            for i in loop_start..=loop_end {
                if segment_configuration[i as usize].kind == Type::Step {
                    has_step_segments_inside_loop = true;
                    break;
                }
            }
        }

        for i in 0..=last_segment {
            let kind = segment_configuration[i].kind;
            let mut s = Segment::default();
            if kind == Type::Ramp {
                s.start = if num_segments == 1 { Some(Field::One) } else { None };
                s.time = Some(Field::Primary(i));
                s.curve = Field::Secondary(i);
                s.portamento = Field::Zero;
                s.phase = None;

                if i == last_segment {
                    s.end = Field::Zero;
                } else if segment_configuration[i + 1].kind != Type::Ramp {
                    s.end = Field::Primary(i + 1);
                } else if i as i32 == first_ramp_segment {
                    s.end = Field::One;
                } else {
                    s.end = Field::Secondary(i);
                    s.curve = Field::Half;
                }
            } else {
                s.start = Some(Field::Primary(i));
                s.end = Field::Primary(i);
                s.curve = Field::Half;
                if kind == Type::Step {
                    s.portamento = Field::Secondary(i);
                    s.time = None;
                    s.phase = if i as i32 == loop_start && i as i32 == loop_end {
                        Some(Field::Zero)
                    } else {
                        Some(Field::One)
                    };
                } else {
                    s.portamento = Field::Zero;
                    s.time = if i as i32 == loop_start && i as i32 == loop_end {
                        None
                    } else {
                        Some(Field::Secondary(i))
                    };
                    s.phase = Some(Field::One);
                }
            }

            s.if_complete = if i as i32 == loop_end { loop_start } else { i as i32 + 1 };
            s.if_falling = if loop_end == -1 || loop_end == last_segment as i32 || has_step_segments {
                -1
            } else {
                loop_end + 1
            };
            s.if_rising = 0;

            if has_step_segments {
                if !has_step_segments_inside_loop && i as i32 >= loop_start && i as i32 <= loop_end {
                    s.if_rising = (loop_end + 1) % num_segments as i32;
                } else {
                    let mut follow_loop = loop_end != -1;
                    let mut next_step = i as i32;
                    while segment_configuration[next_step as usize].kind != Type::Step {
                        next_step += 1;
                        if follow_loop && next_step == loop_end + 1 {
                            next_step = loop_start;
                            follow_loop = false;
                        }
                        if next_step >= num_segments as i32 {
                            next_step = num_segments as i32 - 1;
                            break;
                        }
                    }
                    s.if_rising = if next_step == loop_end {
                        loop_start
                    } else {
                        (next_step + 1) % num_segments as i32
                    };
                }
            }

            self.segments[i] = s;
        }

        let last_end = self.segments[num_segments - 1].end;
        self.segments[num_segments] = Segment {
            start: Some(last_end),
            end: last_end,
            time: Some(Field::Zero),
            curve: Field::Half,
            portamento: Field::Zero,
            phase: None,
            if_rising: 0,
            if_falling: -1,
            if_complete: if loop_end == last_segment as i32 { 0 } else { -1 },
        };

        self.previous_segment = num_segments as i32;
        self.active_segment = num_segments as i32;
    }
}
