//! The "ant memory node": a small, persistent, per-peer tensor of local
//! state. Pure Rust, zero external dependencies — fits the project's
//! single-binary principle. Updated via an exponential moving average, not
//! backprop/training; see the separate `oag-brain` crate (outside this
//! workspace) for the real neural computation that programs these pads.

/// Fixed width of every tensor pad. Deliberately small ("a *small* tensor
/// pad") — this is a lightweight memory slot, not a general embedding store.
pub const TENSOR_PAD_DIM: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub struct TensorPad {
    pub values: Vec<f32>,
}

impl Default for TensorPad {
    fn default() -> Self {
        Self { values: vec![0.0; TENSOR_PAD_DIM] }
    }
}

impl TensorPad {
    /// Build a pad from arbitrary-length values, zero-padding or truncating
    /// to `TENSOR_PAD_DIM` so every pad in the system is always exactly this
    /// width regardless of where it came from.
    pub fn from_values(mut values: Vec<f32>) -> Self {
        values.resize(TENSOR_PAD_DIM, 0.0);
        Self { values }
    }

    /// Nudge every element toward `signal` by `learning_rate` (exponential
    /// moving average: `v = (1-lr)*v + lr*s`). `signal` shorter than
    /// `TENSOR_PAD_DIM` is treated as zero-padded; longer is truncated.
    pub fn update(&mut self, signal: &[f32], learning_rate: f32) {
        for (i, slot) in self.values.iter_mut().enumerate() {
            let s = signal.get(i).copied().unwrap_or(0.0);
            *slot = (1.0 - learning_rate) * *slot + learning_rate * s;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_pad_is_zeroed_and_correctly_sized() {
        let pad = TensorPad::default();
        assert_eq!(pad.values.len(), TENSOR_PAD_DIM);
        assert!(pad.values.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn from_values_pads_short_input_with_zeros() {
        let pad = TensorPad::from_values(vec![1.0, 2.0, 3.0]);
        assert_eq!(pad.values.len(), TENSOR_PAD_DIM);
        assert_eq!(&pad.values[..3], &[1.0, 2.0, 3.0]);
        assert!(pad.values[3..].iter().all(|&v| v == 0.0));
    }

    #[test]
    fn from_values_truncates_long_input() {
        let pad = TensorPad::from_values(vec![1.0; TENSOR_PAD_DIM + 10]);
        assert_eq!(pad.values.len(), TENSOR_PAD_DIM);
    }

    #[test]
    fn update_moves_values_toward_signal_proportional_to_learning_rate() {
        let mut pad = TensorPad::default();
        let signal = vec![1.0; TENSOR_PAD_DIM];
        pad.update(&signal, 0.5);
        assert!(pad.values.iter().all(|&v| (v - 0.5).abs() < 1e-6));

        // A second update with the same signal and rate halves the
        // remaining distance to the target again: 0.5 -> 0.75.
        pad.update(&signal, 0.5);
        assert!(pad.values.iter().all(|&v| (v - 0.75).abs() < 1e-6));
    }

    #[test]
    fn update_with_zero_learning_rate_is_a_no_op() {
        let mut pad = TensorPad::from_values(vec![1.0, 2.0, 3.0]);
        let before = pad.clone();
        pad.update(&[9.0; TENSOR_PAD_DIM], 0.0);
        assert_eq!(pad, before);
    }

    #[test]
    fn update_with_short_signal_treats_missing_elements_as_zero() {
        let mut pad = TensorPad::from_values(vec![1.0]);
        pad.update(&[1.0], 0.5);
        // Index 0 moved toward 1.0 (unchanged, already there); every other
        // index moved toward the implicit zero signal, i.e. stayed at 0.0.
        assert!((pad.values[0] - 1.0).abs() < 1e-6);
        assert!(pad.values[1..].iter().all(|&v| v == 0.0));
    }
}
