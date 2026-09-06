//! `plaits/dsp/voice.h` + `plaits/dsp/voice.cc` -- ties all 24 synthesis
//! models together behind one `Patch`/`Modulations` -> stereo-frame
//! interface: engine selection (with hysteresis, so a CV wiggling near a
//! boundary doesn't chatter), trigger handling (delayed by 1ms, with an
//! internal decay/LPG envelope pair for when no external envelope is
//! patched), and the final limiter/low-pass-gate output stage.
//!
//! # Deviations from the C
//!
//! - `LoadUserData` always receives `None`: nothing in this workspace wires
//!   flash storage into the voice, so the `UserData::ptr(engine_index)` /
//!   `fm_patches_table[]` fallback lookups in the C's `Render` are dropped.
//!   This only matters for [`crate::engines::SixOpEngine`] (stub) and the
//!   wavetable/wave-terrain engines' optional user tables, which are
//!   likewise always `None` here.
//! - Engine slots 2-4 (`SixOpEngine`, registered 3 times in the C against a
//!   single shared instance, differentiated only by which FM patch bank
//!   `LoadUserData` gave it) collapse to the same stub with no behavioral
//!   difference between the three slots -- see the deviation above.

use stmlib::units::semitones_to_ratio;
use stmlib::{DelayLine, HysteresisQuantizer2, Limiter};

use crate::PostProcessingSettings;
use crate::dsp::INV_SAMPLE_RATE;
use crate::engine::{Engine, EngineParameters, note_to_frequency, trigger_state};
use crate::engines::{
    AdditiveEngine, BassDrumEngine, ChiptuneEngine, ChordEngine, FmEngine, GrainEngine,
    HiHatEngine, ModalEngine, NoiseEngine, ParticleEngine, PhaseDistortionEngine, SixOpEngine,
    SnareDrumEngine, SpeechEngine, StringEngine, StringMachineEngine, SwarmEngine,
    VirtualAnalogEngine, VirtualAnalogVcfEngine, WaveTerrainEngine, WaveshapingEngine,
    WavetableEngine,
};
use crate::envelope::{DecayEnvelope, LpgEnvelope};
use crate::fx::LowPassGate;

pub const MAX_ENGINES: usize = 24;
pub const MAX_TRIGGER_DELAY: usize = 8;
pub const TRIGGER_DELAY: usize = 5;

/// One stereo output sample.
#[derive(Debug, Clone, Copy, Default)]
pub struct Frame {
    pub out: i16,
    pub aux: i16,
}

/// The final gain/limiter/LPG stage for one of the two output channels.
#[derive(Default, Debug)]
pub struct ChannelPostProcessor {
    limiter: Limiter,
    lpg: LowPassGate,
}

impl ChannelPostProcessor {
    pub fn init(&mut self) {
        self.lpg.init();
        self.reset();
    }

    pub fn reset(&mut self) {
        self.limiter.init();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        gain: f32,
        bypass_lpg: bool,
        low_pass_gate_gain: f32,
        low_pass_gate_frequency: f32,
        low_pass_gate_hf_bleed: f32,
        in_out: &mut [f32],
    ) {
        if gain < 0.0 {
            self.limiter.process(-gain, in_out);
        }
        let post_gain = if gain < 0.0 { 1.0 } else { gain };
        if !bypass_lpg {
            self.lpg.process(
                post_gain * low_pass_gate_gain,
                low_pass_gate_frequency,
                low_pass_gate_hf_bleed,
                in_out,
            );
        } else {
            for o in in_out.iter_mut() {
                *o *= post_gain;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn process_i16(
        &mut self,
        gain: f32,
        bypass_lpg: bool,
        low_pass_gate_gain: f32,
        low_pass_gate_frequency: f32,
        low_pass_gate_hf_bleed: f32,
        input: &mut [f32],
        out: &mut [i16],
    ) {
        if gain < 0.0 {
            self.limiter.process(-gain, input);
        }
        let post_gain = (if gain < 0.0 { 1.0 } else { gain }) * -32767.0;
        if !bypass_lpg {
            self.lpg.process_to_i16(
                post_gain * low_pass_gate_gain,
                low_pass_gate_frequency,
                low_pass_gate_hf_bleed,
                input,
                out,
                1,
            );
        } else {
            for (o, &x) in out.iter_mut().zip(input.iter()) {
                *o = stmlib::fdsp::clip16(1 + (x * post_gain) as i32) as i16;
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Patch {
    // -- note number in the range from `-119.0` to `120.0`. Default is `48.0`.
    pub note: f32,
    // -- HARMONICS parameter in the range from `0.0` to `1.0`. Default is `0.5`.
    pub harmonics: f32,
    // -- TIMBRE parameter in the range from `0.0` to `1.0`. Default is `0.5`.
    pub timbre: f32,
    // -- MORPH parameter in the range from `0.0` to `1.0`. Default is `0.5`.
    pub morph: f32,
    // -- requency modulation amount in the range from `-1.0` to `1.0`. Default is `0.0`.
    pub frequency_modulation_amount: f32,
    // -- TIMBRE modulation amount in the range from `-1.0` to `1.0`. Default is `0.0`.
    pub timbre_modulation_amount: f32,
    // -- MORPH modulation amount in the range from `-1.0` to `1.0`. Default is `0.0`.
    pub morph_modulation_amount: f32,
    // -- engine selection in the range from `0` to `23`. Default is `0`.
    pub engine: i32,
    // -- envelope decay in the range from `0.0` to `1.0`. Default is `0.5`.
    pub decay: f32,
    // -- Low-pass gate color in the range from `0.0` to `1.0`. Default is `0.5`.
    pub lpg_colour: f32,
}

impl Default for Patch {
    fn default() -> Self {
        Self {
            note: 48.0,
            harmonics: 0.5,
            timbre: 0.5,
            morph: 0.5,
            frequency_modulation_amount: 0.0,
            timbre_modulation_amount: 0.0,
            morph_modulation_amount: 0.0,
            engine: 0,
            decay: 0.5,
            lpg_colour: 0.5,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Modulations {
    // -- engine select modulation in the range from `-1.0` to `1.0`. Default is `0.0`.
    pub engine: f32,
    // -- note number modulation in the range from `-119.0` to `120.0`. Default is `0.0`.
    pub note: f32,
    // -- frequency modulation in the range from `-1.0` to `1.0`. Default is `0.0`.
    pub frequency: f32,
    // -- HARMONICS modulation in the range from `-1.0` to `1.0`. Default is `0.0`.
    pub harmonics: f32,
    // -- TIMBRE modulation in the range from `-1.0` to `1.0`. Default is `0.0`.
    pub timbre: f32,
    // -- MORPH modulation in the range from `-1.0` to `1.0`. Default is `0.0`.
    pub morph: f32,
    // -- Trigger signal in the range from `0.0` to `1.0`. Default is `0.0`.
    pub trigger: f32,
    // -- Level modulation in the range from `0.0` to `1.0`. Default is `0.0`.
    pub level: f32,
    // -- Flag if frequency modulation is applied. Default is `false`.
    pub frequency_patched: bool,
    // -- Flag if timbre modulation is applied. Default is `false`.
    pub timbre_patched: bool,
    // -- Flag if morph modulation is applied. Default is `false`.
    pub morph_patched: bool,
    // -- Flag if trigger signal is used. Default is `false`.
    pub trigger_patched: bool,
    // -- Flag if level modulation is used. Default is `false`.
    pub level_patched: bool,
}

/// `ApplyModulations` in the C (a private `Voice` method there; a free
/// function here since it borrows nothing from `Voice`).
#[allow(clippy::too_many_arguments)]
fn apply_modulations(
    base_value: f32,
    mut modulation_amount: f32,
    use_external_modulation: bool,
    external_modulation: f32,
    use_internal_envelope: bool,
    envelope: f32,
    default_internal_modulation: f32,
    minimum_value: f32,
    maximum_value: f32,
) -> f32 {
    let mut value = base_value;
    modulation_amount *= (modulation_amount.abs() - 0.05).max(0.05);
    modulation_amount *= 1.05;

    let modulation = if use_external_modulation {
        external_modulation
    } else if use_internal_envelope {
        envelope
    } else {
        default_internal_modulation
    };
    value += modulation_amount * modulation;
    value.clamp(minimum_value, maximum_value)
}

#[derive(Default, Debug)]
pub struct Voice<'a> {
    virtual_analog_engine: VirtualAnalogEngine,
    waveshaping_engine: WaveshapingEngine,
    fm_engine: FmEngine,
    grain_engine: GrainEngine,
    additive_engine: AdditiveEngine,
    wavetable_engine: WavetableEngine,
    chord_engine: ChordEngine,
    speech_engine: SpeechEngine<'a>,

    swarm_engine: SwarmEngine,
    noise_engine: NoiseEngine,
    particle_engine: ParticleEngine,
    string_engine: StringEngine,
    modal_engine: ModalEngine,
    bass_drum_engine: BassDrumEngine,
    snare_drum_engine: SnareDrumEngine,
    hi_hat_engine: HiHatEngine,

    virtual_analog_vcf_engine: VirtualAnalogVcfEngine,
    phase_distortion_engine: PhaseDistortionEngine,
    six_op_engine: SixOpEngine,
    wave_terrain_engine: WaveTerrainEngine,
    string_machine_engine: StringMachineEngine,
    chiptune_engine: ChiptuneEngine,

    engine_quantizer: HysteresisQuantizer2,

    reload_user_data: bool,
    previous_engine_index: i32,
    engine_cv: f32,

    previous_note: f32,
    trigger_state: bool,

    decay_envelope: DecayEnvelope,
    lpg_envelope: LpgEnvelope,

    trigger_delay: DelayLine<MAX_TRIGGER_DELAY>,

    out_post_processor: ChannelPostProcessor,
    aux_post_processor: ChannelPostProcessor,
}

impl Voice<'_> {
    pub fn new(block_size: usize) -> Self {
        Self {
            virtual_analog_engine: VirtualAnalogEngine::new(block_size),
            waveshaping_engine: WaveshapingEngine::default(),
            fm_engine: FmEngine::default(),
            grain_engine: GrainEngine::default(),
            additive_engine: AdditiveEngine::default(),
            wavetable_engine: WavetableEngine::default(),
            chord_engine: ChordEngine::default(),
            speech_engine: SpeechEngine::new(block_size),

            swarm_engine: SwarmEngine::default(),
            noise_engine: NoiseEngine::new(block_size),
            particle_engine: ParticleEngine::default(),
            string_engine: StringEngine::new(block_size),
            modal_engine: ModalEngine::new(block_size),
            bass_drum_engine: BassDrumEngine::default(),
            snare_drum_engine: SnareDrumEngine::default(),
            hi_hat_engine: HiHatEngine::new(block_size),

            virtual_analog_vcf_engine: VirtualAnalogVcfEngine::default(),
            phase_distortion_engine: PhaseDistortionEngine::new(block_size),
            six_op_engine: SixOpEngine::new(block_size),
            wave_terrain_engine: WaveTerrainEngine::new(block_size),
            string_machine_engine: StringMachineEngine::default(),
            chiptune_engine: ChiptuneEngine::default(),

            engine_quantizer: HysteresisQuantizer2::default(),

            reload_user_data: false,
            previous_engine_index: -1,
            engine_cv: 0.0,

            previous_note: 0.0,
            trigger_state: false,

            decay_envelope: DecayEnvelope::default(),
            lpg_envelope: LpgEnvelope::default(),

            trigger_delay: DelayLine::default(),

            out_post_processor: ChannelPostProcessor::default(),
            aux_post_processor: ChannelPostProcessor::default(),
        }
    }

    /// Engine-index -> concrete engine dispatch (the Rust equivalent of the
    /// C's `EngineRegistry<24>`, whose `RegisterInstance` calls set up this
    /// exact same index -> instance mapping).
    fn engine_mut(&mut self, index: i32) -> &mut dyn Engine {
        match index {
            0 => &mut self.virtual_analog_vcf_engine,
            1 => &mut self.phase_distortion_engine,
            2..=4 => &mut self.six_op_engine,
            5 => &mut self.wave_terrain_engine,
            6 => &mut self.string_machine_engine,
            7 => &mut self.chiptune_engine,
            8 => &mut self.virtual_analog_engine,
            9 => &mut self.waveshaping_engine,
            10 => &mut self.fm_engine,
            11 => &mut self.grain_engine,
            12 => &mut self.additive_engine,
            13 => &mut self.wavetable_engine,
            14 => &mut self.chord_engine,
            15 => &mut self.speech_engine,
            16 => &mut self.swarm_engine,
            17 => &mut self.noise_engine,
            18 => &mut self.particle_engine,
            19 => &mut self.string_engine,
            20 => &mut self.modal_engine,
            21 => &mut self.bass_drum_engine,
            22 => &mut self.snare_drum_engine,
            _ => &mut self.hi_hat_engine,
        }
    }

    pub fn init(&mut self) {
        self.virtual_analog_engine.init();
        self.waveshaping_engine.init();
        self.fm_engine.init();
        self.grain_engine.init();
        self.additive_engine.init();
        self.wavetable_engine.init();
        self.chord_engine.init();
        self.speech_engine.init();

        self.swarm_engine.init();
        self.noise_engine.init();
        self.particle_engine.init();
        self.string_engine.init();
        self.modal_engine.init();
        self.bass_drum_engine.init();
        self.snare_drum_engine.init();
        self.hi_hat_engine.init();

        self.virtual_analog_vcf_engine.init();
        self.phase_distortion_engine.init();
        self.six_op_engine.init();
        self.wave_terrain_engine.init();
        self.string_machine_engine.init();
        self.chiptune_engine.init();

        self.engine_quantizer.init(MAX_ENGINES as i32, 0.05, true);
        self.previous_engine_index = -1;
        self.reload_user_data = false;
        self.engine_cv = 0.0;

        self.out_post_processor.init();
        self.aux_post_processor.init();

        self.decay_envelope.init();
        self.lpg_envelope.init();

        self.trigger_state = false;
        self.previous_note = 0.0;

        self.trigger_delay.init();
    }

    pub fn reload_user_data(&mut self) {
        self.reload_user_data = true;
    }

    #[inline]
    pub fn active_engine(&self) -> i32 {
        self.previous_engine_index
    }

    fn render_without_postprocessors(
        &mut self,
        patch: &Patch,
        modulations: &Modulations,
        out: &mut [f32],
        aux: &mut [f32],
    ) -> (PostProcessingSettings, bool) {
        // Delay trigger by 1ms to deal with sequencers or MIDI interfaces
        // whose CV out lags behind the GATE out.
        self.trigger_delay.write(modulations.trigger);
        let trigger_value = self.trigger_delay.read_at(TRIGGER_DELAY);

        let previous_trigger_state = self.trigger_state;
        if !previous_trigger_state {
            if trigger_value > 0.3 {
                self.trigger_state = true;
                if !modulations.level_patched {
                    self.lpg_envelope.trigger();
                }
                self.decay_envelope.trigger();
                self.engine_cv = modulations.engine;
            }
        } else if trigger_value < 0.1 {
            self.trigger_state = false;
        }
        if !modulations.trigger_patched {
            self.engine_cv = modulations.engine;
        }

        // Engine selection.
        let engine_index = self
            .engine_quantizer
            .process_base(patch.engine, self.engine_cv)
            .clamp(0, MAX_ENGINES as i32);

        if engine_index != self.previous_engine_index || self.reload_user_data {
            match engine_index {
                2 => {
                    self.six_op_engine
                        .load_syx_bank(&crate::resources::SYX_BANK_0);
                }
                3 => {
                    self.six_op_engine
                        .load_syx_bank(&crate::resources::SYX_BANK_1);
                }
                4 => {
                    self.six_op_engine
                        .load_syx_bank(&crate::resources::SYX_BANK_2);
                }
                5 => {
                    self.wave_terrain_engine
                        .load_user_data(Some(&crate::resources::SYX_BANK_0));
                }
                13 => {
                    self.wavetable_engine
                        .set_wavetables(&crate::resources::WAV_INTEGRATED_WAVES);
                }
                _ => {}
            }
            self.engine_mut(engine_index).reset();

            self.out_post_processor.reset();
            self.previous_engine_index = engine_index;
            self.reload_user_data = false;
        }

        let mut p = EngineParameters::default();

        let rising_edge = self.trigger_state && !previous_trigger_state;
        let note = (modulations.note + self.previous_note) * 0.5;
        self.previous_note = modulations.note;

        if modulations.trigger_patched {
            p.trigger = if rising_edge {
                trigger_state::RISING_EDGE
            } else if self.trigger_state {
                trigger_state::HIGH
            } else {
                trigger_state::LOW
            };
        } else {
            p.trigger = trigger_state::UNPATCHED;
        }

        let short_decay = (200.0 * out.len() as f32)
            * INV_SAMPLE_RATE
            * semitones_to_ratio(-96.0 * patch.decay.clamp(0.1, 1.0));
        self.decay_envelope.process(short_decay * 2.0);

        let compressed_level =
            (1.3 * modulations.level / (0.3 + modulations.level.abs())).clamp(0.0, 1.0);
        p.accent = if modulations.level_patched {
            compressed_level
        } else {
            0.8
        };

        let use_internal_envelope = modulations.trigger_patched;

        // Actual synthesis parameters.
        p.harmonics = (patch.harmonics + modulations.harmonics).clamp(0.0, 1.0);

        let mut internal_envelope_amplitude = 1.0f32;
        let mut internal_envelope_amplitude_timbre = 1.0f32;
        if engine_index == 15 {
            internal_envelope_amplitude = (2.0 - p.harmonics * 6.0).clamp(0.0, 1.0);
            self.speech_engine.set_prosody_amount(
                if !modulations.trigger_patched || modulations.frequency_patched {
                    0.0
                } else {
                    patch.frequency_modulation_amount
                },
            );
            self.speech_engine.set_speed(
                if !modulations.trigger_patched || modulations.morph_patched {
                    0.0
                } else {
                    patch.morph_modulation_amount
                },
            );
        } else if engine_index == 7 {
            if modulations.trigger_patched && !modulations.timbre_patched {
                // Disable internal envelope on TIMBRE, and enable the
                // envelope generator built into the chiptune engine.
                internal_envelope_amplitude_timbre = 0.0;
                self.chiptune_engine
                    .set_envelope_shape(patch.timbre_modulation_amount);
            } else {
                self.chiptune_engine
                    .set_envelope_shape(crate::engines::chiptune_engine::NO_ENVELOPE);
            }
        }

        p.note = apply_modulations(
            patch.note + note,
            patch.frequency_modulation_amount,
            modulations.frequency_patched,
            modulations.frequency,
            use_internal_envelope,
            internal_envelope_amplitude
                * self.decay_envelope.value()
                * self.decay_envelope.value()
                * 48.0,
            1.0,
            -119.0,
            120.0,
        );

        p.timbre = apply_modulations(
            patch.timbre,
            patch.timbre_modulation_amount,
            modulations.timbre_patched,
            modulations.timbre,
            use_internal_envelope,
            internal_envelope_amplitude_timbre * self.decay_envelope.value(),
            0.0,
            0.0,
            1.0,
        );

        p.morph = apply_modulations(
            patch.morph,
            patch.morph_modulation_amount,
            modulations.morph_patched,
            modulations.morph,
            use_internal_envelope,
            internal_envelope_amplitude * self.decay_envelope.value(),
            0.0,
            0.0,
            1.0,
        );

        let pp_s: PostProcessingSettings = self.engine_mut(engine_index).post_processing_settings();

        let already_enveloped =
            self.engine_mut(engine_index)
                .render(&p, out, aux, pp_s.already_enveloped);

        let lpg_bypass =
            already_enveloped || (!modulations.level_patched && !modulations.trigger_patched);

        // -- compute LPG parameters.
        if !lpg_bypass {
            let hf = patch.lpg_colour;
            let decay_tail = (20.0 * out.len() as f32)
                * INV_SAMPLE_RATE
                * semitones_to_ratio(-72.0 * patch.decay + 12.0 * hf)
                - short_decay;

            if modulations.level_patched {
                self.lpg_envelope
                    .process_lp(compressed_level, short_decay, decay_tail, hf);
            } else {
                let attack = note_to_frequency(p.note) * out.len() as f32 * 2.0;
                self.lpg_envelope
                    .process_ping(attack, short_decay, decay_tail, hf);
            }
        } else {
            self.lpg_envelope.init();
        }

        (pp_s, lpg_bypass)
    }

    pub fn render(
        &mut self,
        patch: &Patch,
        modulations: &Modulations,
        out_buffer: &mut [f32],
        aux_buffer: &mut [f32],
    ) {
        let (pp_s, lpg_bypass) =
            self.render_without_postprocessors(patch, modulations, out_buffer, aux_buffer);

        self.out_post_processor.process(
            pp_s.out_gain,
            lpg_bypass,
            self.lpg_envelope.gain(),
            self.lpg_envelope.frequency(),
            self.lpg_envelope.hf_bleed(),
            out_buffer,
        );

        self.aux_post_processor.process(
            pp_s.aux_gain,
            lpg_bypass,
            self.lpg_envelope.gain(),
            self.lpg_envelope.frequency(),
            self.lpg_envelope.hf_bleed(),
            aux_buffer,
        );
    }

    pub fn render_frames(
        &mut self,
        patch: &Patch,
        modulations: &Modulations,
        out_buffer: &mut [f32],
        aux_buffer: &mut [f32],
        out_i16: &mut [i16],
        aux_i16: &mut [i16],
        frames: &mut [Frame],
    ) {
        self.render_i16(patch, modulations, out_buffer, aux_buffer, out_i16, aux_i16);
        for (frame, (&o, &a)) in frames.iter_mut().zip(out_i16.iter().zip(aux_i16.iter())) {
            frame.out = o;
            frame.aux = a;
        }
    }

    pub fn render_i16(
        &mut self,
        patch: &Patch,
        modulations: &Modulations,
        out_buffer: &mut [f32],
        aux_buffer: &mut [f32],
        out_i16: &mut [i16],
        aux_i16: &mut [i16],
    ) {
        let (pp_s, lpg_bypass) =
            self.render_without_postprocessors(patch, modulations, out_buffer, aux_buffer);

        self.out_post_processor.process_i16(
            pp_s.out_gain,
            lpg_bypass,
            self.lpg_envelope.gain(),
            self.lpg_envelope.frequency(),
            self.lpg_envelope.hf_bleed(),
            out_buffer,
            out_i16,
        );

        self.aux_post_processor.process_i16(
            pp_s.aux_gain,
            lpg_bypass,
            self.lpg_envelope.gain(),
            self.lpg_envelope.frequency(),
            self.lpg_envelope.hf_bleed(),
            aux_buffer,
            aux_i16,
        );
    }
}
