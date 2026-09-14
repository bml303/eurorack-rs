//! `streams/processor.{h,cc}` -- `Processor`: dispatches to one of the 6
//! dynamics-processing algorithms, all sharing the same `init`/`process`/
//! `configure` shape. The C dispatches through a `ProcessorCallbacks`
//! function-pointer table (`callbacks_table_`); this port just holds every
//! algorithm as a field and `match`es on [`ProcessorFunction`] instead.

use crate::compressor::Compressor;
use crate::envelope::Envelope;
use crate::filter_controller::FilterController;
use crate::follower::Follower;
use crate::lorenz_generator::LorenzGenerator;
use crate::vactrol::Vactrol;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProcessorFunction {
    #[default]
    Envelope,
    Vactrol,
    Follower,
    Compressor,
    FilterController,
    LorenzGenerator,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Processor {
    function: ProcessorFunction,
    linked: bool,
    alternate: bool,
    dirty: bool,
    parameters: [i32; 2],
    globals: [i32; 4],

    last_gain_value: u16,
    last_frequency_value: u16,

    envelope: Envelope,
    vactrol: Vactrol,
    follower: Follower,
    compressor: Compressor,
    filter_controller: FilterController,
    lorenz_generator: LorenzGenerator,
}

impl Processor {
    pub fn new() -> Self {
        let mut p = Self::default();
        p.init();
        p
    }

    pub fn init(&mut self) {
        self.init_with_index(0);
    }

    pub fn init_with_index(&mut self, index: u8) {
        self.envelope.init();
        self.vactrol.init();
        self.follower.init();
        self.compressor.init();
        self.filter_controller.init();
        self.lorenz_generator.init();

        self.dirty = true;
        self.alternate = false;
        self.linked = false;

        self.parameters = [32768; 2];
        self.globals = [32768; 4];

        self.set_function(ProcessorFunction::Envelope);

        self.lorenz_generator.set_index(index);
    }

    pub fn set_function(&mut self, function: ProcessorFunction) {
        self.function = function;
        match function {
            ProcessorFunction::Envelope => self.envelope.init(),
            ProcessorFunction::Vactrol => self.vactrol.init(),
            ProcessorFunction::Follower => self.follower.init(),
            ProcessorFunction::Compressor => self.compressor.init(),
            ProcessorFunction::FilterController => self.filter_controller.init(),
            ProcessorFunction::LorenzGenerator => self.lorenz_generator.init(),
        }
        self.dirty = true;
    }

    pub fn set_alternate(&mut self, alternate: bool) {
        self.alternate = alternate;
        self.dirty = true;
    }

    pub fn set_linked(&mut self, linked: bool) {
        self.linked = linked;
        self.dirty = true;
    }

    pub fn set_parameter(&mut self, index: usize, value: u16) {
        self.parameters[index] = value as i32;
        self.dirty = true;
    }

    pub fn set_global(&mut self, index: usize, value: u16) {
        self.globals[index] = value as i32;
        self.dirty = self.linked;
    }

    pub fn function(&self) -> ProcessorFunction {
        self.function
    }
    pub fn alternate(&self) -> bool {
        self.alternate
    }
    pub fn linked(&self) -> bool {
        self.linked
    }
    pub fn last_frequency(&self) -> u8 {
        (self.last_frequency_value >> 8) as u8
    }
    pub fn last_gain(&self) -> u8 {
        (self.last_gain_value >> 8) as u8
    }
    pub fn gain_reduction(&self) -> i32 {
        self.compressor.gain_reduction()
    }

    /// Returns `(gain, frequency)`.
    pub fn process(&mut self, audio: i16, excite: i16) -> (u16, u16) {
        let mut gain = 0u16;
        let mut frequency = 0u16;
        match self.function {
            ProcessorFunction::Envelope => self.envelope.process(audio, excite, &mut gain, &mut frequency),
            ProcessorFunction::Vactrol => self.vactrol.process(audio, excite, &mut gain, &mut frequency),
            ProcessorFunction::Follower => self.follower.process(audio, excite, &mut gain, &mut frequency),
            ProcessorFunction::Compressor => self.compressor.process(audio, excite, &mut gain, &mut frequency),
            ProcessorFunction::FilterController => self.filter_controller.process(audio, excite, &mut gain, &mut frequency),
            ProcessorFunction::LorenzGenerator => self.lorenz_generator.process(audio, excite, &mut gain, &mut frequency),
        }
        self.last_gain_value = gain;
        self.last_frequency_value = frequency;
        (gain, frequency)
    }

    pub fn configure(&mut self) {
        if !self.dirty {
            return;
        }
        let globals = if self.linked { Some(&self.globals) } else { None };
        match self.function {
            ProcessorFunction::Envelope => self.envelope.configure(self.alternate, &self.parameters, globals),
            ProcessorFunction::Vactrol => self.vactrol.configure(self.alternate, &self.parameters, globals),
            ProcessorFunction::Follower => self.follower.configure(self.alternate, &self.parameters, globals),
            ProcessorFunction::Compressor => self.compressor.configure(self.alternate, &self.parameters, globals),
            ProcessorFunction::FilterController => self.filter_controller.configure(self.alternate, &self.parameters, globals),
            ProcessorFunction::LorenzGenerator => self.lorenz_generator.configure(self.alternate, &self.parameters, globals),
        }
        self.dirty = false;
    }
}
