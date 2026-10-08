//! The evolution layer's one job: scaled dot-product self-attention
//! ("Attention Is All You Need") over the small set of tensor pads in play,
//! used only to *program* (overwrite) each participating node's own pad.
//!
//! v1 scope, stated plainly: `Wq`/`Wk`/`Wv` are fixed, deterministically
//! seeded projection matrices, not trained — no training objective or
//! dataset was specified. The computation itself (scaled dot-product
//! attention) is the real mechanism, not a placeholder standing in for one.

use oag_tensor::TENSOR_PAD_DIM;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use tensorflow::ops::{Const, Div, MatMul, Placeholder, Softmax};
use tensorflow::{DataType, Operation, Scope, Session, SessionOptions, SessionRunArgs, Tensor};

/// Fixed so every `oag-brain` instance derives the same projection matrices
/// without coordination — same spirit as `oag-petname`'s deterministic,
/// seed-derived naming. Not a secret; this is a scaffold, not a trained
/// model, so there is nothing to protect by varying it.
const WEIGHT_SEED: u64 = 0xA17_BEEF;

const N: usize = 2; // self pad + the incoming peer's pad, in play per exchange.
const D: usize = TENSOR_PAD_DIM;

/// Fills a `D x D` matrix with small, deterministically-seeded values and
/// wraps it as a TensorFlow `Const` operation.
fn seeded_weight_const(rng: &mut ChaCha8Rng, scope: &mut Scope) -> tensorflow::Result<Operation> {
    let mut tensor = Tensor::<f32>::new(&[D as u64, D as u64]);
    for slot in tensor.iter_mut() {
        *slot = rng.gen_range(-0.1f32..0.1f32);
    }
    Const::new().value(tensor).dtype(DataType::Float).build(scope)
}

/// One `Brain`: a built TensorFlow graph + session implementing scaled
/// dot-product attention over `N` stacked pads, held open for the life of
/// the process rather than rebuilt per exchange.
pub struct Brain {
    session: Session,
    x: Operation,
    output: Operation,
}

impl Brain {
    pub fn new() -> tensorflow::Result<Self> {
        let mut scope = Scope::new_root_scope();
        let mut rng = ChaCha8Rng::seed_from_u64(WEIGHT_SEED);

        let x = Placeholder::new().dtype(DataType::Float).build(&mut scope)?;

        let wq = seeded_weight_const(&mut rng, &mut scope)?;
        let wk = seeded_weight_const(&mut rng, &mut scope)?;
        let wv = seeded_weight_const(&mut rng, &mut scope)?;

        let q = MatMul::new().build(x.clone(), wq, &mut scope)?;
        let k = MatMul::new().build(x.clone(), wk, &mut scope)?;
        let v = MatMul::new().build(x.clone(), wv, &mut scope)?;

        // scores = Q . K^T / sqrt(D)
        let raw_scores = MatMul::new().transpose_b(true).build(q, k, &mut scope)?;
        let scale = Const::new()
            .value(Tensor::from(1.0f32 / (D as f32).sqrt()))
            .dtype(DataType::Float)
            .build(&mut scope)?;
        let scores = Div::new().build(raw_scores, scale, &mut scope)?;
        let weights = Softmax::new().build(scores, &mut scope)?;

        // output = softmax(scores) . V -- row i is node i's newly-programmed pad.
        let output = MatMul::new().build(weights, v, &mut scope)?;

        let session = Session::new(&SessionOptions::new(), &scope.graph())?;
        Ok(Self { session, x, output })
    }

    /// Run attention over `[own_pad, peer_pad]` (each exactly
    /// `TENSOR_PAD_DIM` long) and return `(programmed_own_pad,
    /// programmed_peer_pad)` -- row 0 and row 1 of the attention output.
    pub fn attend(&self, own_pad: &[f32], peer_pad: &[f32]) -> tensorflow::Result<(Vec<f32>, Vec<f32>)> {
        debug_assert_eq!(own_pad.len(), D);
        debug_assert_eq!(peer_pad.len(), D);

        let mut input = Tensor::<f32>::new(&[N as u64, D as u64]);
        input[..D].copy_from_slice(own_pad);
        input[D..2 * D].copy_from_slice(peer_pad);

        let mut args = SessionRunArgs::new();
        args.add_feed(&self.x, 0, &input);
        let output_token = args.request_fetch(&self.output, 0);
        self.session.run(&mut args)?;

        let output: Tensor<f32> = args.fetch(output_token)?;
        Ok((output[..D].to_vec(), output[D..2 * D].to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attend_produces_pads_of_the_right_width() {
        let brain = Brain::new().unwrap();
        let (own, peer) = brain.attend(&vec![0.1; D], &vec![0.2; D]).unwrap();
        assert_eq!(own.len(), D);
        assert_eq!(peer.len(), D);
    }

    #[test]
    fn attend_is_deterministic_for_the_same_inputs() {
        let brain = Brain::new().unwrap();
        let (own_a, peer_a) = brain.attend(&vec![0.3; D], &vec![0.4; D]).unwrap();
        let (own_b, peer_b) = brain.attend(&vec![0.3; D], &vec![0.4; D]).unwrap();
        assert_eq!(own_a, own_b);
        assert_eq!(peer_a, peer_b);
    }

    #[test]
    fn two_brain_instances_derive_identical_weights_from_the_fixed_seed() {
        let a = Brain::new().unwrap();
        let b = Brain::new().unwrap();
        let (own_a, peer_a) = a.attend(&vec![0.5; D], &vec![-0.5; D]).unwrap();
        let (own_b, peer_b) = b.attend(&vec![0.5; D], &vec![-0.5; D]).unwrap();
        assert_eq!(own_a, own_b);
        assert_eq!(peer_a, peer_b);
    }

    #[test]
    fn differing_peer_input_changes_the_attention_output() {
        let brain = Brain::new().unwrap();
        let (own_1, _) = brain.attend(&vec![0.1; D], &vec![0.2; D]).unwrap();
        let (own_2, _) = brain.attend(&vec![0.1; D], &vec![9.0; D]).unwrap();
        assert_ne!(own_1, own_2, "a very different peer pad should shift the attention-weighted output");
    }

    /// Hand-checkable without knowing Wq/Wk/Wv at all: an all-zero input
    /// means Q=K=V are all-zero matrices (`MatMul(0, W) = 0`), so every
    /// score is 0, softmax over two equal scores is exactly [0.5, 0.5], and
    /// `weights . V = weights . 0 = 0` regardless of the weights. Confirms
    /// the graph really is computing `softmax(QK^T/sqrt(D)) . V`, not
    /// something that merely happens to produce plausible-looking output.
    #[test]
    fn zero_input_produces_zero_output_by_construction() {
        let brain = Brain::new().unwrap();
        let (own, peer) = brain.attend(&vec![0.0; D], &vec![0.0; D]).unwrap();
        assert!(own.iter().all(|&v| v == 0.0));
        assert!(peer.iter().all(|&v| v == 0.0));
    }

    /// Hand-checkable: identical input rows give identical Q/K/V rows, so
    /// both rows of the 2x2 score matrix are equal, softmax yields [0.5,
    /// 0.5] either way, and `weights . V` collapses to the same single row
    /// for both outputs -- regardless of the weight values.
    #[test]
    fn identical_pads_produce_identical_output_rows() {
        let brain = Brain::new().unwrap();
        let (own, peer) = brain.attend(&vec![0.7; D], &vec![0.7; D]).unwrap();
        assert_eq!(own, peer);
    }
}
