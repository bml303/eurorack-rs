//! Shape enumerations.
//!
//! Braids has a subtlety worth spelling out: [`DigitalOscillator`] is dispatched
//! by the *numeric* value `macro_shape - TripleRingMod`, and the C
//! `fn_table_` in `digital_oscillator.cc` is ordered to match the **tail of
//! [`MacroOscillatorShape`]**, not the (stale, partly-misnamed)
//! `DigitalOscillatorShape` enum in the C header. [`DigitalModel`] below uses
//! the authoritative `fn_table_` order with corrected names.
//!
//! [`DigitalOscillator`]: crate::digital_oscillator::DigitalOscillator

/// `MacroOscillatorShape` from `braids/settings.h` -- the 48 user-selectable
/// models, in firmware order (the discriminants are the stored setting values).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum MacroOscillatorShape {
    Csaw = 0,
    Morph,
    SawSquare,
    SineTriangle,
    Buzz,
    SquareSub,
    SawSub,
    SquareSync,
    SawSync,
    TripleSaw,
    TripleSquare,
    TripleTriangle,
    TripleSine,
    TripleRingMod,
    SawSwarm,
    SawComb,
    Toy,
    DigitalFilterLp,
    DigitalFilterPk,
    DigitalFilterBp,
    DigitalFilterHp,
    Vosim,
    Vowel,
    VowelFof,
    Harmonics,
    Fm,
    FeedbackFm,
    ChaoticFeedbackFm,
    Plucked,
    Bowed,
    Blown,
    Fluted,
    StruckBell,
    StruckDrum,
    Kick,
    Cymbal,
    Snare,
    Wavetables,
    WaveMap,
    WaveLine,
    WaveParaphonic,
    FilteredNoise,
    TwinPeaksNoise,
    ClockedNoise,
    GranularCloud,
    ParticleNoise,
    DigitalModulation,
    QuestionMark,
}

impl MacroOscillatorShape {
    pub const COUNT: usize = 48;

    /// All shapes, ascending.
    pub const ALL: [MacroOscillatorShape; Self::COUNT] = {
        use MacroOscillatorShape::*;
        [
            Csaw,
            Morph,
            SawSquare,
            SineTriangle,
            Buzz,
            SquareSub,
            SawSub,
            SquareSync,
            SawSync,
            TripleSaw,
            TripleSquare,
            TripleTriangle,
            TripleSine,
            TripleRingMod,
            SawSwarm,
            SawComb,
            Toy,
            DigitalFilterLp,
            DigitalFilterPk,
            DigitalFilterBp,
            DigitalFilterHp,
            Vosim,
            Vowel,
            VowelFof,
            Harmonics,
            Fm,
            FeedbackFm,
            ChaoticFeedbackFm,
            Plucked,
            Bowed,
            Blown,
            Fluted,
            StruckBell,
            StruckDrum,
            Kick,
            Cymbal,
            Snare,
            Wavetables,
            WaveMap,
            WaveLine,
            WaveParaphonic,
            FilteredNoise,
            TwinPeaksNoise,
            ClockedNoise,
            GranularCloud,
            ParticleNoise,
            DigitalModulation,
            QuestionMark,
        ]
    };

    #[inline]
    pub fn from_u8(v: u8) -> Option<Self> {
        Self::ALL.get(v as usize).copied()
    }
}

/// The digital synthesis models, in `digital_oscillator.cc::fn_table_` order
/// (== `MacroOscillatorShape` from [`TripleRingMod`] onward).
///
/// [`TripleRingMod`]: MacroOscillatorShape::TripleRingMod
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum DigitalModel {
    TripleRingMod = 0,
    SawSwarm,
    Comb,
    Toy,
    DigitalFilterLp,
    DigitalFilterPk,
    DigitalFilterBp,
    DigitalFilterHp,
    Vosim,
    Vowel,
    VowelFof,
    Harmonics,
    Fm,
    FeedbackFm,
    ChaoticFeedbackFm,
    Plucked,
    Bowed,
    Blown,
    Fluted,
    StruckBell,
    StruckDrum,
    Kick,
    Cymbal,
    Snare,
    Wavetables,
    WaveMap,
    WaveLine,
    WaveParaphonic,
    FilteredNoise,
    TwinPeaksNoise,
    ClockedNoise,
    GranularCloud,
    ParticleNoise,
    DigitalModulation,
    QuestionMark,
}

impl DigitalModel {
    pub const COUNT: usize = 35;

    pub const ALL: [DigitalModel; Self::COUNT] = {
        use DigitalModel::*;
        [
            TripleRingMod,
            SawSwarm,
            Comb,
            Toy,
            DigitalFilterLp,
            DigitalFilterPk,
            DigitalFilterBp,
            DigitalFilterHp,
            Vosim,
            Vowel,
            VowelFof,
            Harmonics,
            Fm,
            FeedbackFm,
            ChaoticFeedbackFm,
            Plucked,
            Bowed,
            Blown,
            Fluted,
            StruckBell,
            StruckDrum,
            Kick,
            Cymbal,
            Snare,
            Wavetables,
            WaveMap,
            WaveLine,
            WaveParaphonic,
            FilteredNoise,
            TwinPeaksNoise,
            ClockedNoise,
            GranularCloud,
            ParticleNoise,
            DigitalModulation,
            QuestionMark,
        ]
    };

    #[inline]
    pub fn from_u8(v: u8) -> Option<Self> {
        Self::ALL.get(v as usize).copied()
    }
}

/// `AnalogOscillatorShape` from `braids/analog_oscillator.h`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AnalogOscillatorShape {
    Saw = 0,
    VariableSaw,
    Csaw,
    Square,
    Triangle,
    Sine,
    TriangleFold,
    SineFold,
    Buzz,
}
