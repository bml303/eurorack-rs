//! `peaks/processors.{h,cc}` -- `Processors`: dispatches to one of the 12
//! processor functions, all sharing the same `init`/`process`/`configure`
//! shape. The C dispatches through a `ProcessorCallbacks` function-pointer
//! table (`callbacks_table_`); this port just holds every algorithm as a
//! field and `match`es on [`ProcessorFunction`] instead.
//!
//! `TapLfo` is not a separate engine -- it's the same [`Lfo`] instance with
//! `set_sync(true)`, matching the C's `lfo_.set_sync(function ==
//! PROCESSOR_FUNCTION_TAP_LFO)` and its skip of `LfoInit()` on that specific
//! transition (switching in and out of tap-sync mode doesn't reset the
//! LFO's phase/pattern-predictor state).

use crate::drums::bass_drum::BassDrum;
use crate::drums::fm_drum::FmDrum;
use crate::drums::high_hat::HighHat;
use crate::drums::snare_drum::SnareDrum;
use crate::gate_processor::{ControlMode, GateFlags};
use crate::modulations::bouncing_ball::BouncingBall;
use crate::modulations::lfo::Lfo;
use crate::modulations::mini_sequencer::MiniSequencer;
use crate::modulations::multistage_envelope::MultistageEnvelope;
use crate::number_station::NumberStation;
use crate::pulse_processor::pulse_randomizer::PulseRandomizer;
use crate::pulse_processor::pulse_shaper::PulseShaper;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProcessorFunction {
    #[default]
    Envelope,
    Lfo,
    TapLfo,
    BassDrum,
    SnareDrum,
    HighHat,
    FmDrum,
    PulseShaper,
    PulseRandomizer,
    BouncingBall,
    MiniSequencer,
    NumberStation,
}

pub struct Processors {
    control_mode: ControlMode,
    function: ProcessorFunction,
    parameter: [u16; 4],

    envelope: MultistageEnvelope,
    lfo: Lfo,
    bass_drum: BassDrum,
    snare_drum: SnareDrum,
    high_hat: HighHat,
    fm_drum: FmDrum,
    pulse_shaper: PulseShaper,
    pulse_randomizer: PulseRandomizer,
    bouncing_ball: BouncingBall,
    mini_sequencer: MiniSequencer,
    number_station: NumberStation,
}

impl Default for Processors {
    fn default() -> Self {
        Self {
            control_mode: ControlMode::Full,
            function: ProcessorFunction::Envelope,
            // Zero, not 32768: the C++'s `processors[2]` is a zero-initialized
            // static array, and `Init()` sets `function_`/calls its first
            // `Configure()` *before* filling `parameter_` with 32768 -- so
            // that first configure pass genuinely runs on zeros. Matched here
            // by defaulting to zero too, rather than pre-seeding a "sensible"
            // value that `init()` doesn't actually see.
            parameter: [0; 4],
            envelope: MultistageEnvelope::default(),
            lfo: Lfo::default(),
            bass_drum: BassDrum::default(),
            snare_drum: SnareDrum::default(),
            high_hat: HighHat::default(),
            fm_drum: FmDrum::default(),
            pulse_shaper: PulseShaper::default(),
            pulse_randomizer: PulseRandomizer::default(),
            bouncing_ball: BouncingBall::default(),
            mini_sequencer: MiniSequencer::default(),
            number_station: NumberStation::default(),
        }
    }
}

impl Processors {
    pub fn new() -> Self {
        let mut p = Self::default();
        p.init(0);
        p
    }

    pub fn init(&mut self, index: u8) {
        self.envelope.init();
        self.lfo.init();
        self.bass_drum.init();
        self.snare_drum.init();
        self.fm_drum.init();
        self.fm_drum.set_sd_range(index == 1);
        self.high_hat.init();
        self.bouncing_ball.init();
        self.pulse_shaper.init();
        self.pulse_randomizer.init();
        self.mini_sequencer.init();
        self.number_station.init();
        self.number_station.set_voice(index == 1);

        self.control_mode = ControlMode::Full;
        self.set_function(ProcessorFunction::Envelope);
        self.parameter = [32768; 4];
    }

    pub fn set_control_mode(&mut self, control_mode: ControlMode) {
        self.control_mode = control_mode;
        self.configure();
    }

    pub fn set_parameter(&mut self, index: usize, parameter: u16) {
        self.parameter[index] = parameter;
        self.configure();
    }

    pub fn copy_parameters(&mut self, parameters: &[u16]) {
        self.parameter[..parameters.len()].copy_from_slice(parameters);
    }

    pub fn set_function(&mut self, function: ProcessorFunction) {
        self.function = function;
        self.lfo.set_sync(function == ProcessorFunction::TapLfo);
        if function != ProcessorFunction::TapLfo {
            self.init_current();
        }
        self.configure();
    }

    pub fn function(&self) -> ProcessorFunction {
        self.function
    }

    pub fn number_station(&self) -> &NumberStation {
        &self.number_station
    }

    fn init_current(&mut self) {
        match self.function {
            ProcessorFunction::Envelope => self.envelope.init(),
            ProcessorFunction::Lfo | ProcessorFunction::TapLfo => self.lfo.init(),
            ProcessorFunction::BassDrum => self.bass_drum.init(),
            ProcessorFunction::SnareDrum => self.snare_drum.init(),
            ProcessorFunction::HighHat => self.high_hat.init(),
            ProcessorFunction::FmDrum => self.fm_drum.init(),
            ProcessorFunction::PulseShaper => self.pulse_shaper.init(),
            ProcessorFunction::PulseRandomizer => self.pulse_randomizer.init(),
            ProcessorFunction::BouncingBall => self.bouncing_ball.init(),
            ProcessorFunction::MiniSequencer => self.mini_sequencer.init(),
            ProcessorFunction::NumberStation => self.number_station.init(),
        }
    }

    pub fn process(&mut self, gate_flags: &[GateFlags], out: &mut [i16]) {
        match self.function {
            ProcessorFunction::Envelope => self.envelope.process(gate_flags, out),
            ProcessorFunction::Lfo | ProcessorFunction::TapLfo => self.lfo.process(gate_flags, out),
            ProcessorFunction::BassDrum => self.bass_drum.process(gate_flags, out),
            ProcessorFunction::SnareDrum => self.snare_drum.process(gate_flags, out),
            ProcessorFunction::HighHat => self.high_hat.process(gate_flags, out),
            ProcessorFunction::FmDrum => self.fm_drum.process(gate_flags, out),
            ProcessorFunction::PulseShaper => self.pulse_shaper.process(gate_flags, out),
            ProcessorFunction::PulseRandomizer => self.pulse_randomizer.process(gate_flags, out),
            ProcessorFunction::BouncingBall => self.bouncing_ball.process(gate_flags, out),
            ProcessorFunction::MiniSequencer => self.mini_sequencer.process(gate_flags, out),
            ProcessorFunction::NumberStation => self.number_station.process(gate_flags, out),
        }
    }

    fn configure(&mut self) {
        // Auto-switch between SnareDrum and HighHat when both "tone" and
        // "snappy" are cranked near their maximum -- the front panel has no
        // dedicated hi-hat mode switch, so this substitutes for one.
        if self.function == ProcessorFunction::SnareDrum || self.function == ProcessorFunction::HighHat {
            let tone_parameter = if self.control_mode == ControlMode::Full { self.parameter[1] } else { self.parameter[0] };
            let snappy_parameter = if self.control_mode == ControlMode::Full { self.parameter[2] } else { self.parameter[1] };
            if tone_parameter >= 65000 && snappy_parameter >= 65000 {
                if self.function != ProcessorFunction::HighHat {
                    self.set_function(ProcessorFunction::HighHat);
                }
            } else if (tone_parameter <= 64500 || snappy_parameter <= 64500) && self.function != ProcessorFunction::SnareDrum {
                self.set_function(ProcessorFunction::SnareDrum);
            }
        }

        let parameter = self.parameter;
        let control_mode = self.control_mode;
        match self.function {
            ProcessorFunction::Envelope => self.envelope.configure(&parameter, control_mode),
            ProcessorFunction::Lfo | ProcessorFunction::TapLfo => self.lfo.configure(&parameter, control_mode),
            ProcessorFunction::BassDrum => self.bass_drum.configure(&parameter, control_mode),
            ProcessorFunction::SnareDrum => self.snare_drum.configure(&parameter, control_mode),
            ProcessorFunction::HighHat => self.high_hat.configure(&parameter, control_mode),
            ProcessorFunction::FmDrum => self.fm_drum.configure(&parameter, control_mode),
            ProcessorFunction::PulseShaper => self.pulse_shaper.configure(&parameter, control_mode),
            ProcessorFunction::PulseRandomizer => self.pulse_randomizer.configure(&parameter, control_mode),
            ProcessorFunction::BouncingBall => self.bouncing_ball.configure(&parameter, control_mode),
            ProcessorFunction::MiniSequencer => self.mini_sequencer.configure(&parameter, control_mode),
            ProcessorFunction::NumberStation => self.number_station.configure(&parameter, control_mode),
        }
    }
}
