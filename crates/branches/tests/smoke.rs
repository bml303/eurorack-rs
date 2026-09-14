//! Crash / sanity smoke test: `Branches`/`Channel` must survive a long
//! trigger/threshold/mode sweep without panicking, and the gate decision
//! must behave exactly as `branches.cc`'s core loop specifies at its
//! deterministic edges (threshold 0 always passes, threshold 65535 always
//! blocks, toggle mode flip-flops, latch mode holds through the falling
//! edge). `Rng` must never get stuck.

use branches::{probability_to_threshold, Branches, Channel};

#[test]
fn threshold_zero_always_passes_and_max_always_blocks() {
    let mut always_pass = Channel::new();
    let mut always_block = Channel::new();

    for step in 0..2000u32 {
        let input = step % 2 == 0;
        // A varying random draw shouldn't matter at either extreme.
        let random = ((step * 2654435761) % 65536) as u16;
        always_pass.step(input, random, 0);
        always_block.step(input, random, 65535);
        if input {
            assert!(always_pass.gate(), "threshold 0 should always let a rising edge through");
        }
        assert!(!always_block.gate(), "threshold 65535 should always block, regardless of the draw");
    }
}

#[test]
fn toggle_mode_flip_flops_on_successive_rising_edges() {
    let mut channel = Channel::new();
    channel.toggle_mode = true;

    // threshold 0 makes the pre-XOR outcome deterministically `true`, so
    // toggle mode should alternate the gate every rising edge.
    let mut expected = false;
    for press in 0..40 {
        expected = !expected;
        // rising edge
        channel.step(true, 0, 0);
        assert_eq!(channel.gate(), expected, "press #{press}: toggle mode should have flipped");
        // falling edge (non-latch: gate always follows input low, but state
        // was already captured on the rising edge above)
        channel.step(false, 0, 0);
    }
}

#[test]
fn latch_mode_holds_through_the_falling_edge_non_latch_does_not() {
    let mut latched = Channel::new();
    latched.latch_mode = true;
    let mut unlatched = Channel::new();

    latched.step(true, 0, 0); // rising edge, threshold 0 -> gate on
    unlatched.step(true, 0, 0);
    assert!(latched.gate());
    assert!(unlatched.gate());

    latched.step(false, 0, 0); // falling edge
    unlatched.step(false, 0, 0);
    assert!(latched.gate(), "latch mode should hold the gate through the falling edge");
    assert!(!unlatched.gate(), "non-latch mode should drop the gate on the falling edge");
}

#[test]
fn branches_survives_a_long_two_channel_sweep() {
    let mut branches = Branches::new();

    for step in 0..20_000i32 {
        branches.channel[0].toggle_mode = step % 5 == 0;
        branches.channel[0].latch_mode = step % 7 == 0;
        branches.channel[1].toggle_mode = step % 3 == 0;
        branches.channel[1].latch_mode = step % 11 == 0;

        let input = [step % 4 == 0, step % 6 == 0];
        let threshold = [probability_to_threshold((step % 256) as u8), probability_to_threshold(((step * 3) % 256) as u8)];
        let gates = branches.step(input, threshold);
        assert_eq!(gates, [branches.channel[0].gate(), branches.channel[1].gate()]);
    }
}

#[test]
fn probability_to_threshold_is_monotonic_and_spans_the_full_range() {
    assert_eq!(probability_to_threshold(0), 0);
    assert_eq!(probability_to_threshold(255), 65535);
    let mut previous = 0u16;
    for adc in 0..=255u8 {
        let threshold = probability_to_threshold(adc);
        assert!(threshold >= previous, "linear_table should be non-decreasing");
        previous = threshold;
    }
}

#[test]
fn rng_never_gets_stuck_at_zero() {
    use branches::rng::Rng;
    let mut rng = Rng::new();
    for _ in 0..200_000 {
        let words = rng.next_words();
        assert_ne!(words, 0, "the LFSR should never settle at the absorbing all-zero state from a seed of 1");
    }
}
