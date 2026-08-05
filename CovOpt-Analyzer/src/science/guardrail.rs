/// The generic interface for Token-level Guardrails (0/1 Tree Pruning)
///
/// This interface allows the application layer (like kyberna) to hook into ModelGo's
/// generation loop and enforce strict semantic/syntactic constraints.
pub trait TokenGuardrail {
    /// Applies a mask to the logits before the LLM samples the next token.
    /// This is where you implement 0/1 pruning by setting invalid logits to `-f32::INFINITY`.
    fn apply_mask(&mut self, current_token: u32, logits: &mut [f32]);

    /// Advances the internal state machine after a token has been definitively generated/sampled.
    fn advance_state(&mut self, generated_token: u32);
}

/// A No-Op guardrail for when no constraints are applied.
pub struct NoOpGuardrail;

impl TokenGuardrail for NoOpGuardrail {
    #[inline(always)]
    fn apply_mask(&mut self, _current_token: u32, _logits: &mut [f32]) {
        // Do nothing, all tokens are allowed.
    }

    #[inline(always)]
    fn advance_state(&mut self, _generated_token: u32) {
        // No state to advance.
    }
}
