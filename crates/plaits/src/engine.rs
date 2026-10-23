//! `plaits/dsp/engine/engine.h` -- the interface every synthesis model
//! implements, plus the small bits of shared control-rate logic.

use stmlib::units::semitones_to_ratio;

use crate::dsp::A0;

/// `NoteToFrequency(midi_note)` -- MIDI note number (69 = A4) to normalised
/// frequency (cycles/sample).
#[inline]
pub fn note_to_frequency(midi_note: f32) -> f32 {
    let midi_note = (midi_note - 9.0).clamp(-128.0, 127.0);
    A0 * 0.25 * semitones_to_ratio(midi_note)
}

/// `TriggerState` -- the C OR's `TRIGGER_RISING_EDGE`/`TRIGGER_HIGH` together;
/// kept as a plain bitmask for the same reason.
pub mod trigger_state {
    pub const LOW: i32 = 0;
    pub const RISING_EDGE: i32 = 1;
    pub const UNPATCHED: i32 = 2;
    pub const HIGH: i32 = 4;
}

#[derive(Debug, Clone, Copy)]
pub struct EngineParameters {
    pub trigger: i32,
    pub note: f32,
    pub timbre: f32,
    pub morph: f32,
    pub harmonics: f32,
    pub accent: f32,
    /// A fourth synthesis macro, 0..1 with 0.5 the stock sound: from the
    /// Plaits Palette alternative firmware (Rubato, lylepmills/eurorack),
    /// not Mutable's, where the FREQUENCY knob drives it with the pitch
    /// locked. Each engine gives it its own meaning; `SixOpEngine`,
    /// `WavetableEngine` and `WaveTerrainEngine` implement it here, the
    /// rest ignore it.
    pub fourth_macro: f32,
}

impl Default for EngineParameters {
    fn default() -> Self {
        Self {
            trigger: 0,
            note: 0.0,
            timbre: 0.0,
            morph: 0.0,
            harmonics: 0.0,
            accent: 0.0,
            fourth_macro: 0.5,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PostProcessingSettings {
    /// A negative value means "route through the limiter" (see `Voice`).
    pub out_gain: f32,
    pub aux_gain: f32,
    /// The engine already applies its own envelope (a modal drum, an 808
    /// kick, a spoken word) -- `Voice` should bypass the LPG for it.
    pub already_enveloped: bool,
}

/// `plaits::Engine` -- implemented by each of the 24 synthesis models.
///
/// `render` returns whether the rendered block is already enveloped,
/// overriding `PostProcessingSettings::already_enveloped` for this call (used
/// by `SpeechEngine`, which alternates between a continuous vowel -- needs the
/// LPG -- and a spoken word -- already has its own contour).
pub trait Engine {
    fn init(&mut self);
    fn reset(&mut self);
    fn load_user_data(&mut self, user_data: Option<&'static [u8]>);
    fn render(
        &mut self,
        parameters: &EngineParameters,
        out: &mut [f32],
        aux: &mut [f32],
        already_enveloped: bool,
    ) -> bool;
    fn post_processing_settings(&self) -> PostProcessingSettings;
}
