//! `plaits/dsp/fm/algorithms.h` -- the DX7/DX100 FM algorithm graphs (which
//! operator modulates which, and which ones sum into the audible output),
//! and a small "compiler" that turns each algorithm's raw per-operator
//! opcode into a sequence of [`RenderCall`]s.
//!
//! Each operator's opcode packs where it reads its phase-modulation input
//! from (`SOURCE`: none, a numbered buffer, or feedback) and which numbered
//! buffer it writes (`DESTINATION`), plus whether it *adds* to that buffer's
//! existing content rather than overwriting it, and (for a feedback source)
//! which specific operator of the chain the feedback is tapped from. Adjacent
//! operators that chain together with no other operator branching off the
//! same buffer in between are compiled into a single [`super::operator::
//! render_operators`] call over all of them at once (`RendererSpecs`, keyed
//! by how many operators, the resolved modulation source, and additive-ness)
//! -- if no such call exists for a chain (the C's `renderers_` table only
//! covers a handful of `n`/`modulation_source` combinations actually needed
//! by DX7/DX100 algorithms), it falls back to one call per operator.
//!
//! `NUM_OPERATORS`/`NUM_ALGORITHMS` replace the C's `Algorithms<num_operators>`
//! template (with `NumAlgorithms<n>` mapping 4 -> 8, 6 -> 32): this port only
//! ever needs `Algorithms<6, 32>` (the six-op engine's DX7 tables; the DX100's
//! 4-operator/8-algorithm set, `NUM_OPERATORS == 4`, is unreachable dead
//! code here since nothing in this port instantiates it), but keeps both
//! const-generic parameters so `opcodes()`/`renderers()` can dispatch on them
//! the same way the C's template specialisation did.

use super::operator::{render_operators, RenderFn};

const DESTINATION_MASK: u8 = 0x03;
const SOURCE_MASK: u8 = 0x30;
const SOURCE_FEEDBACK: u8 = 0x30;
const ADDITIVE_FLAG: u8 = 0x04;
const FEEDBACK_SOURCE_FLAG: u8 = 0x40;

const fn modulated_by(buffer: u8) -> u8 {
    buffer << 4
}
const fn additive(destination: u8) -> u8 {
    destination | ADDITIVE_FLAG
}
const NOT_MODULATED: u8 = modulated_by(0);
/// This operator's result is the algorithm's audible output (mixed
/// additively, since several operators can be summed into it).
const OUTPUT: u8 = additive(0);
const FEEDBACK_DESTINATION: u8 = modulated_by(3);
/// This operator both reads and writes the feedback loop's carry-over state
/// (`fb_state`) -- see [`super::operator::render_operators`].
const FEEDBACK_SOURCE: u8 = FEEDBACK_SOURCE_FLAG | FEEDBACK_DESTINATION;

#[derive(Debug, Clone, Copy, Default)]
pub struct RenderCall {
    pub render_fn: Option<RenderFn>,
    pub n: usize,
    pub input_index: usize,
    pub output_index: usize,
}

struct RendererSpecs {
    n: usize,
    modulation_source: i32,
    additive: bool,
    render_fn: RenderFn,
}

macro_rules! renderer {
    ($n:literal, $modulation_source:literal, $additive:literal) => {
        RendererSpecs {
            n: $n,
            modulation_source: $modulation_source,
            additive: $additive,
            render_fn: render_operators::<$n, $modulation_source, $additive>,
        }
    };
}

/// The handful of (operator count, modulation source, additive) combinations
/// DX7/DX100 algorithms actually need a fused multi-operator render call
/// for. The C's `renderers_` table also lists several wider ("Optimized")
/// combinations, all commented out there (unused) -- dropped here.
const RENDERERS_6: [RendererSpecs; 8] = [
    renderer!(1, -2, false),
    renderer!(1, -2, true),
    renderer!(1, -1, false),
    renderer!(1, -1, true),
    renderer!(1, 0, false),
    renderer!(1, 0, true),
    // Feedback loops spanning several operators.
    renderer!(3, 2, true),
    renderer!(2, 1, true),
];

const RENDERERS_4: [RendererSpecs; 6] = [
    renderer!(1, -2, false),
    renderer!(1, -2, true),
    renderer!(1, -1, false),
    renderer!(1, -1, true),
    renderer!(1, 0, false),
    renderer!(1, 0, true),
];

#[derive(Debug)]
pub struct Algorithms<const NUM_OPERATORS: usize, const NUM_ALGORITHMS: usize> {
    render_call: [[RenderCall; NUM_OPERATORS]; NUM_ALGORITHMS],
}

impl<const NUM_OPERATORS: usize, const NUM_ALGORITHMS: usize> Default
    for Algorithms<NUM_OPERATORS, NUM_ALGORITHMS>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<const NUM_OPERATORS: usize, const NUM_ALGORITHMS: usize>
    Algorithms<NUM_OPERATORS, NUM_ALGORITHMS>
{
    pub fn new() -> Self {
        Self {
            render_call: [[RenderCall::default(); NUM_OPERATORS]; NUM_ALGORITHMS],
        }
    }

    pub fn init(&mut self) {
        for algorithm in 0..NUM_ALGORITHMS {
            self.compile(algorithm);
        }
    }

    pub fn render_call(&self, algorithm: u8, op: usize) -> &RenderCall {
        &self.render_call[algorithm as usize][op]
    }

    pub fn is_modulator(&self, algorithm: u8, op: usize) -> bool {
        self.opcode(algorithm, op) & DESTINATION_MASK != 0
    }

    fn opcode(&self, algorithm: u8, op: usize) -> u8 {
        opcodes::<NUM_OPERATORS, NUM_ALGORITHMS>()[algorithm as usize][op]
    }

    fn renderers(&self) -> &'static [RendererSpecs] {
        if NUM_OPERATORS == 6 {
            &RENDERERS_6
        } else {
            &RENDERERS_4
        }
    }

    fn get_renderer(&self, n: usize, modulation_source: i32, additive: bool) -> Option<RenderFn> {
        self.renderers()
            .iter()
            .find(|r| r.n == n && r.modulation_source == modulation_source && r.additive == additive)
            .map(|r| r.render_fn)
    }

    /// Turns one algorithm's raw opcodes into a sequence of [`RenderCall`]s,
    /// greedily grouping adjacent operators into the widest chain a
    /// precompiled renderer exists for.
    fn compile(&mut self, algorithm: usize) {
        let mut i = 0;
        while i < NUM_OPERATORS {
            let opcode = self.opcode(algorithm as u8, i);

            // Extend the chain while each next operator's source buffer is
            // exactly the previous operator's destination (a straight-line
            // modulation chain), and the previous operator doesn't also mix
            // additively into a *different* buffer (branching off the chain).
            let mut n = 1;
            while i + n < NUM_OPERATORS {
                let from = self.opcode(algorithm as u8, i + n - 1);
                let to = (self.opcode(algorithm as u8, i + n) & SOURCE_MASK) >> 4;
                let has_additive = from & ADDITIVE_FLAG != 0;
                let broken = (from & DESTINATION_MASK) != to;
                if has_additive || broken {
                    if to == (opcode & DESTINATION_MASK) {
                        // The same modulation is reused by a later operator
                        // (algorithms 19-25) -- discard the chain rather than
                        // apply it twice.
                        n = 1;
                    }
                    break;
                }
                n += 1;
            }

            // Find a precompiled renderer for this chain, shrinking it to a
            // single operator (which always has one, per `RENDERERS_*`) if
            // none exists for the wider grouping.
            for _attempt in 0..2 {
                let out_opcode = self.opcode(algorithm as u8, i + n - 1);
                let out_additive = out_opcode & ADDITIVE_FLAG != 0;

                let modulation_source = if opcode & SOURCE_MASK == 0 {
                    -1
                } else if opcode & SOURCE_MASK != SOURCE_FEEDBACK {
                    -2
                } else {
                    (0..n)
                        .find(|&j| self.opcode(algorithm as u8, i + j) & FEEDBACK_SOURCE_FLAG != 0)
                        .expect("a feedback-sourced chain always tags its feedback operator")
                        as i32
                };

                if let Some(render_fn) = self.get_renderer(n, modulation_source, out_additive) {
                    self.render_call[algorithm][i] = RenderCall {
                        render_fn: Some(render_fn),
                        n,
                        input_index: ((opcode & SOURCE_MASK) >> 4) as usize,
                        output_index: (out_opcode & DESTINATION_MASK) as usize,
                    };
                    break;
                }
                debug_assert_ne!(n, 1, "every algorithm needs a single-operator renderer");
                n = 1;
            }

            i += n;
        }
    }
}

/// `Algorithms<4>::opcodes_` (from the DX100) / `Algorithms<6>::opcodes_`
/// (from the DX7), selected by `NUM_OPERATORS`/`NUM_ALGORITHMS` the same way
/// the C's template specialisation did.
fn opcodes<const NUM_OPERATORS: usize, const NUM_ALGORITHMS: usize>() -> &'static [[u8; 6]] {
    // Both tables are stored as `[u8; 6]` rows regardless of `NUM_OPERATORS`
    // (the DX100 table's unused trailing 2 columns are zero, i.e. `NO_MOD |
    // OUT(0)`, and never read since `NUM_OPERATORS == 4` iteration stops at
    // column 4) -- this keeps one array type instead of duplicating
    // `Algorithms::compile`/`opcode` per width.
    if NUM_OPERATORS == 6 {
        &OPCODES_6
    } else {
        &OPCODES_4_PADDED
    }
}

#[rustfmt::skip]
const OPCODES_4: [[u8; 4]; 8] = [
    [ // Algorithm 1: 4 -> 3 -> 2 -> 1
        FEEDBACK_SOURCE | 1, modulated_by(1) | 1, modulated_by(1) | 1, modulated_by(1) | OUTPUT,
    ],
    [ // Algorithm 2: 4 + 3 -> 2 -> 1
        FEEDBACK_SOURCE | 1, additive(1), modulated_by(1) | 1, modulated_by(1) | OUTPUT,
    ],
    [ // Algorithm 3: 4 + (3 -> 2) -> 1
        FEEDBACK_SOURCE | 1, 2, modulated_by(2) | additive(1), modulated_by(1) | OUTPUT,
    ],
    [ // Algorithm 4: (4 -> 3) + 2 -> 1
        FEEDBACK_SOURCE | 1, modulated_by(1) | 1, additive(1), modulated_by(1) | OUTPUT,
    ],
    [ // Algorithm 5: (4 -> 3) + (2 -> 1)
        FEEDBACK_SOURCE | 1, modulated_by(1) | OUTPUT, 1, modulated_by(1) | additive(0),
    ],
    [ // Algorithm 6: (4 -> 3) + (4 -> 2) + (4 -> 1)
        FEEDBACK_SOURCE | 1, modulated_by(1) | OUTPUT, modulated_by(1) | additive(0), modulated_by(1) | additive(0),
    ],
    [ // Algorithm 7: (4 -> 3) + 2 + 1
        FEEDBACK_SOURCE | 1, modulated_by(1) | OUTPUT, additive(0), additive(0),
    ],
    [ // Algorithm 8: 4 + 3 + 2 + 1
        FEEDBACK_SOURCE | OUTPUT, additive(0), additive(0), additive(0),
    ],
];

/// [`OPCODES_4`], padded to `[u8; 6]` rows -- see [`opcodes`].
const OPCODES_4_PADDED: [[u8; 6]; 8] = {
    let mut padded = [[NOT_MODULATED; 6]; 8];
    let mut algorithm = 0;
    while algorithm < 8 {
        let mut op = 0;
        while op < 4 {
            padded[algorithm][op] = OPCODES_4[algorithm][op];
            op += 1;
        }
        algorithm += 1;
    }
    padded
};

#[rustfmt::skip]
const OPCODES_6: [[u8; 6]; 32] = [
    [ // Algorithm 1
        FEEDBACK_SOURCE | 1, modulated_by(1) | 1, modulated_by(1) | 1,
        modulated_by(1) | OUTPUT, NOT_MODULATED | 1, modulated_by(1) | additive(0),
    ],
    [ // Algorithm 2
        NOT_MODULATED | 1, modulated_by(1) | 1, modulated_by(1) | 1,
        modulated_by(1) | OUTPUT, FEEDBACK_SOURCE | 1, modulated_by(1) | additive(0),
    ],
    [ // Algorithm 3
        FEEDBACK_SOURCE | 1, modulated_by(1) | 1, modulated_by(1) | OUTPUT,
        NOT_MODULATED | 1, modulated_by(1) | 1, modulated_by(1) | additive(0),
    ],
    [ // Algorithm 4
        FEEDBACK_DESTINATION | NOT_MODULATED | 1, modulated_by(1) | 1, FEEDBACK_SOURCE_FLAG | modulated_by(1) | OUTPUT,
        NOT_MODULATED | 1, modulated_by(1) | 1, modulated_by(1) | additive(0),
    ],
    [ // Algorithm 5
        FEEDBACK_SOURCE | 1, modulated_by(1) | OUTPUT, NOT_MODULATED | 1,
        modulated_by(1) | additive(0), NOT_MODULATED | 1, modulated_by(1) | additive(0),
    ],
    [ // Algorithm 6
        FEEDBACK_DESTINATION | NOT_MODULATED | 1, FEEDBACK_SOURCE_FLAG | modulated_by(1) | OUTPUT, NOT_MODULATED | 1,
        modulated_by(1) | additive(0), NOT_MODULATED | 1, modulated_by(1) | additive(0),
    ],
    [ // Algorithm 7
        FEEDBACK_SOURCE | 1, modulated_by(1) | 1, NOT_MODULATED | additive(1),
        modulated_by(1) | OUTPUT, NOT_MODULATED | 1, modulated_by(1) | additive(0),
    ],
    [ // Algorithm 8
        NOT_MODULATED | 1, modulated_by(1) | 1, FEEDBACK_SOURCE | additive(1),
        modulated_by(1) | OUTPUT, NOT_MODULATED | 1, modulated_by(1) | additive(0),
    ],
    [ // Algorithm 9
        NOT_MODULATED | 1, modulated_by(1) | 1, NOT_MODULATED | additive(1),
        modulated_by(1) | OUTPUT, FEEDBACK_SOURCE | 1, modulated_by(1) | additive(0),
    ],
    [ // Algorithm 10
        NOT_MODULATED | 1, NOT_MODULATED | additive(1), modulated_by(1) | OUTPUT,
        FEEDBACK_SOURCE | 1, modulated_by(1) | 1, modulated_by(1) | additive(0),
    ],
    [ // Algorithm 11
        FEEDBACK_SOURCE | 1, NOT_MODULATED | additive(1), modulated_by(1) | OUTPUT,
        NOT_MODULATED | 1, modulated_by(1) | 1, modulated_by(1) | additive(0),
    ],
    [ // Algorithm 12
        NOT_MODULATED | 1, NOT_MODULATED | additive(1), NOT_MODULATED | additive(1),
        modulated_by(1) | OUTPUT, FEEDBACK_SOURCE | 1, modulated_by(1) | additive(0),
    ],
    [ // Algorithm 13
        FEEDBACK_SOURCE | 1, NOT_MODULATED | additive(1), NOT_MODULATED | additive(1),
        modulated_by(1) | OUTPUT, NOT_MODULATED | 1, modulated_by(1) | additive(0),
    ],
    [ // Algorithm 14
        FEEDBACK_SOURCE | 1, NOT_MODULATED | additive(1), modulated_by(1) | 1,
        modulated_by(1) | OUTPUT, NOT_MODULATED | 1, modulated_by(1) | additive(0),
    ],
    [ // Algorithm 15
        NOT_MODULATED | 1, NOT_MODULATED | additive(1), modulated_by(1) | 1,
        modulated_by(1) | OUTPUT, FEEDBACK_SOURCE | 1, modulated_by(1) | additive(0),
    ],
    [ // Algorithm 16
        FEEDBACK_SOURCE | 1, modulated_by(1) | 1, NOT_MODULATED | 2,
        modulated_by(2) | additive(1), NOT_MODULATED | additive(1), modulated_by(1) | OUTPUT,
    ],
    [ // Algorithm 17
        NOT_MODULATED | 1, modulated_by(1) | 1, NOT_MODULATED | 2,
        modulated_by(2) | additive(1), FEEDBACK_SOURCE | additive(1), modulated_by(1) | OUTPUT,
    ],
    [ // Algorithm 18
        NOT_MODULATED | 1, modulated_by(1) | 1, modulated_by(1) | 1,
        FEEDBACK_SOURCE | additive(1), NOT_MODULATED | additive(1), modulated_by(1) | OUTPUT,
    ],
    [ // Algorithm 19
        FEEDBACK_SOURCE | 1, modulated_by(1) | OUTPUT, modulated_by(1) | additive(0),
        NOT_MODULATED | 1, modulated_by(1) | 1, modulated_by(1) | additive(0),
    ],
    [ // Algorithm 20
        NOT_MODULATED | 1, NOT_MODULATED | additive(1), modulated_by(1) | OUTPUT,
        FEEDBACK_SOURCE | 1, modulated_by(1) | additive(0), modulated_by(1) | additive(0),
    ],
    [ // Algorithm 21
        NOT_MODULATED | 1, modulated_by(1) | OUTPUT, modulated_by(1) | additive(0),
        FEEDBACK_SOURCE | 1, modulated_by(1) | additive(0), modulated_by(1) | additive(0),
    ],
    [ // Algorithm 22
        FEEDBACK_SOURCE | 1, modulated_by(1) | OUTPUT, modulated_by(1) | additive(0),
        modulated_by(1) | additive(0), NOT_MODULATED | 1, modulated_by(1) | additive(0),
    ],
    [ // Algorithm 23
        FEEDBACK_SOURCE | 1, modulated_by(1) | OUTPUT, modulated_by(1) | additive(0),
        NOT_MODULATED | 1, modulated_by(1) | additive(0), NOT_MODULATED | additive(0),
    ],
    [ // Algorithm 24
        FEEDBACK_SOURCE | 1, modulated_by(1) | OUTPUT, modulated_by(1) | additive(0),
        modulated_by(1) | additive(0), NOT_MODULATED | additive(0), NOT_MODULATED | additive(0),
    ],
    [ // Algorithm 25
        FEEDBACK_SOURCE | 1, modulated_by(1) | OUTPUT, modulated_by(1) | additive(0),
        NOT_MODULATED | additive(0), NOT_MODULATED | additive(0), NOT_MODULATED | additive(0),
    ],
    [ // Algorithm 26
        FEEDBACK_SOURCE | 1, NOT_MODULATED | additive(1), modulated_by(1) | OUTPUT,
        NOT_MODULATED | 1, modulated_by(1) | additive(0), NOT_MODULATED | additive(0),
    ],
    [ // Algorithm 27
        NOT_MODULATED | 1, NOT_MODULATED | additive(1), modulated_by(1) | OUTPUT,
        FEEDBACK_SOURCE | 1, modulated_by(1) | additive(0), NOT_MODULATED | additive(0),
    ],
    [ // Algorithm 28
        NOT_MODULATED | OUTPUT, FEEDBACK_SOURCE | 1, modulated_by(1) | 1,
        modulated_by(1) | additive(0), NOT_MODULATED | 1, modulated_by(1) | additive(0),
    ],
    [ // Algorithm 29
        FEEDBACK_SOURCE | 1, modulated_by(1) | OUTPUT, NOT_MODULATED | 1,
        modulated_by(1) | additive(0), NOT_MODULATED | additive(0), NOT_MODULATED | additive(0),
    ],
    [ // Algorithm 30
        NOT_MODULATED | OUTPUT, FEEDBACK_SOURCE | 1, modulated_by(1) | 1,
        modulated_by(1) | additive(0), NOT_MODULATED | additive(0), NOT_MODULATED | additive(0),
    ],
    [ // Algorithm 31
        FEEDBACK_SOURCE | 1, modulated_by(1) | OUTPUT, NOT_MODULATED | additive(0),
        NOT_MODULATED | additive(0), NOT_MODULATED | additive(0), NOT_MODULATED | additive(0),
    ],
    [ // Algorithm 32
        FEEDBACK_SOURCE | OUTPUT, NOT_MODULATED | additive(0), NOT_MODULATED | additive(0),
        NOT_MODULATED | additive(0), NOT_MODULATED | additive(0), NOT_MODULATED | additive(0),
    ],
];
