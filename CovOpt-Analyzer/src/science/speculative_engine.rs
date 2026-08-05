use crate::io::loader::*;
use std::sync::Arc;
use vec101::core::QuantType;

use crate::TieredKVCache;

pub struct SpeculativeEngine {
    pub kv_cache: TieredKVCache,
    pub x_stream: Vec<i8>,
    pub s_stream: Vec<i32>,
    pub out_buffer: Vec<i32>,
    pub loader: Arc<SafetensorsMmapLoader>,
    pub mailbox: Arc<vec101::sync::AtomicMailboxU32>,
}

pub struct DraftTree {
    pub tokens: Vec<u32>,
    pub tree_mask: Vec<u32>, // parent_idx
}

impl SpeculativeEngine {
    pub fn new(
        loader: Arc<SafetensorsMmapLoader>,
        max_batch_size: usize,
        hidden_dim: usize,
        session_id: u32,
    ) -> Self {
        println!("[Trace] SpeculativeEngine::new -> TieredKVCache::new");
        let kv_cache = TieredKVCache::new(session_id, hidden_dim, 64);
        println!("[Trace] SpeculativeEngine::new -> vec! allocations");
        Self {
            kv_cache,
            x_stream: vec![0; max_batch_size * 256000],
            s_stream: vec![1; 256000],
            out_buffer: vec![0; max_batch_size * 256000],
            loader,
            mailbox: Arc::new(vec101::sync::AtomicMailboxU32::new()),
        }
    }

    /// Drafting Phase: Layer skipping MTP with Tree Search
    ///
    /// # Safety
    /// This function dereferences raw pointers inside the zero-copy loader and weights.
    pub unsafe fn run_draft_mode(
        &mut self,
        prompt_tokens: &[u32],
        target_depth: usize,
        layer_stride: usize,
        max_nodes: usize,
    ) -> DraftTree {
        // Embed prompt_tokens into x_stream
        for (i, &tok) in prompt_tokens.iter().take(2048).enumerate() {
            let val = ((tok % 255) as i16 - 128) as i8;
            for d in 0..16 {
                self.x_stream[i * 16 + d] = val;
            }
        }
        let mut tree = DraftTree {
            tokens: Vec::with_capacity(max_nodes),
            tree_mask: Vec::with_capacity(max_nodes),
        };
        let top_k = 2; // Beam width expansion factor
        let mut current_frontier = vec![0u32]; // Start with the prompt tip node index 0
        let mut node_count = 1; // 0 is root (implicit)
        tree.tokens.push(0); // Root token (dummy or prompt tip)
        tree.tree_mask.push(0); // Root parent is itself

        for _depth in 0..target_depth {
            if node_count >= max_nodes || current_frontier.is_empty() {
                break;
            }

            let batch_size = current_frontier.len();

            for idx in 0..32 {
                if idx % layer_stride != 0 {
                    continue; // Skip layer
                }

                let tensor_name = format!("model.layers.{}.weight", idx);
                let w_stream = if let Some(&ptr) = self.loader.tensors.get(&tensor_name) {
                    ptr
                } else {
                    continue;
                };

                let blocks_per_row = self.kv_cache.hidden_dim / 2048;
                let num_rows = 4096;
                let w_slice = unsafe {
                    std::slice::from_raw_parts(
                        w_stream,
                        blocks_per_row * core::mem::size_of::<vec101::core::Vec101SuperBlock>(),
                    )
                };
                let mut engine = vec101::core::Vec101EngineBorrow::new(
                    w_slice,
                    &self.x_stream,
                    &self.s_stream,
                    &mut self.out_buffer,
                    batch_size,
                    num_rows,
                    blocks_per_row,
                )
                .unwrap();
                engine.set_quant_type(QuantType::Bit1_58);
                engine.set_num_threads(0); // FIX: Disable dynamic thread spawning to prevent OS thrashing and deadlocks
                engine.set_tree_mask(current_frontier.as_ptr(), current_frontier.len());
                engine.compute();
            }

            let mut next_frontier = Vec::new();
            for (b, &parent_node_idx) in current_frontier.iter().enumerate().take(batch_size) {
                let logits = &self.out_buffer
                    [b * self.kv_cache.hidden_dim..(b + 1) * self.kv_cache.hidden_dim];

                // Emulate Top-K token extraction
                let mut candidates: Vec<(u32, i32)> = logits
                    .iter()
                    .enumerate()
                    .map(|(i, &v)| (i as u32, v))
                    .collect();
                // Simple partial sort would be better, but sort_unstable_by is sufficient for emulation
                candidates.sort_unstable_by(|a, b| {
                    b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
                });

                for candidate in candidates.iter().take(top_k) {
                    if node_count >= max_nodes {
                        break;
                    }
                    let token_id = candidate.0;
                    tree.tokens.push(token_id);
                    tree.tree_mask.push(parent_node_idx);
                    next_frontier.push(node_count as u32);
                    let _ = self.mailbox.try_push(token_id);

                    // Embed the newly drafted token for the next step (naive autoregressive update)
                    let current_len = prompt_tokens.len() + node_count - 1;
                    if current_len < 2048 {
                        let val = ((token_id % 255) as i16 - 128) as i8;
                        for d in 0..16 {
                            self.x_stream[current_len * 16 + d] = val;
                        }
                    }

                    node_count += 1;
                }
            }
            current_frontier = next_frontier;
        }
        tree
    }

    /// Verify Phase: Compute missing layers with Batch=TreeSize
    ///
    /// # Safety
    /// This function dereferences raw pointers inside the zero-copy loader and weights.
    pub unsafe fn run_verify_mode(
        &mut self,
        draft_tree: &DraftTree,
        layer_stride: usize,
    ) -> Vec<u32> {
        let len = draft_tree.tokens.len();
        for idx in 0..32 {
            // Only process the layers we skipped during draft
            if idx % layer_stride == 0 {
                continue;
            }

            let tensor_name = format!("model.layers.{}.weight", idx);
            let w_stream = if let Some(&ptr) = self.loader.tensors.get(&tensor_name) {
                ptr
            } else {
                continue;
            };

            let blocks_per_row = self.kv_cache.hidden_dim / 2048;
            let num_rows = 4096;
            let w_slice = unsafe {
                std::slice::from_raw_parts(
                    w_stream,
                    blocks_per_row * core::mem::size_of::<vec101::core::Vec101SuperBlock>(),
                )
            };
            // In a real scenario, we fetch the blocks needed for the attention
            let block0 = self.kv_cache.fetch_block(0);
            let mut ptrs = Vec::new();
            if let Some(ref b) = block0 {
                ptrs.push(b.as_ptr());
            }

            let mut engine = vec101::core::Vec101EngineBorrow::new(
                w_slice,
                &self.x_stream,
                &self.s_stream,
                &mut self.out_buffer,
                len, // Batch = Tree Size
                num_rows,
                blocks_per_row,
            )
            .unwrap();
            engine.set_quant_type(QuantType::Bit1_58);
            engine.set_num_threads(0); // FIX: Disable dynamic thread spawning
            engine.set_tree_mask(draft_tree.tree_mask.as_ptr(), len);
            engine.set_kv_blocks(ptrs.as_ptr(), ptrs.len(), 64);
            engine.compute();
        }

        // Extract verified logits for the entire tree batch
        let mut verified_logits = Vec::with_capacity(len);
        for i in 0..len {
            let offset = i * self.kv_cache.hidden_dim;
            let logits = &self.out_buffer[offset..offset + self.kv_cache.hidden_dim];

            let mut max_val = i32::MIN;
            let mut max_idx = 0;
            for (idx, &v) in logits.iter().enumerate() {
                if v > max_val {
                    max_val = v;
                    max_idx = idx as u32;
                }
            }
            verified_logits.push(max_idx);
        }

        // Graph traversal to find longest valid path starting from root
        let mut max_depth = 1;
        let mut best_leaf = 0;
        let mut node_depth = vec![0; len];
        let mut is_valid = vec![false; len];

        is_valid[0] = true; // Root is always valid initially
        node_depth[0] = 1;

        for i in 1..len {
            let p = draft_tree.tree_mask[i] as usize;
            if is_valid[p] && draft_tree.tokens[i] == verified_logits[p] {
                is_valid[i] = true;
                node_depth[i] = node_depth[p] + 1;
                if node_depth[i] > max_depth {
                    max_depth = node_depth[i];
                    best_leaf = i;
                }
            }
        }

        // Backtrack to extract the accepted tokens sequence
        let mut accepted_tokens = Vec::new();
        let mut curr = best_leaf;
        while curr != 0 {
            accepted_tokens.push(draft_tree.tokens[curr]);
            curr = draft_tree.tree_mask[curr] as usize;
        }
        accepted_tokens.reverse();

        // Push the final verified token from the leaf node
        accepted_tokens.push(verified_logits[best_leaf]);

        accepted_tokens
    }
}
