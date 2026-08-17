use crate::science::chaos_state::ChaosEngram;

/// 1-bit Topology Canvas (畫布)
/// 用於取代原先 1.58 bit (w_pos, w_neg) 的張量矩陣。
/// 此畫布只有 1-bit，它的 `0` 代表的是 -1，`1` 代表的是 +1。
/// 連續狀態則由 ChaosEngram 動態推進來達成期望值。
#[derive(Clone, Debug)]
pub struct TopologyCanvas {
    pub width: usize,
    pub height: usize,
    pub bitmask: Vec<u64>,
}

impl TopologyCanvas {
    pub fn new(width: usize, height: usize) -> Self {
        let blocks = (width * height).div_ceil(64);
        Self {
            width,
            height,
            bitmask: vec![0; blocks],
        }
    }

    /// 根據已知的混沌態 (ChaosEngram) 推進畫布
    /// 這相當於執行 Cellular Automata 或 Stochastic Computing 的一步
    pub fn advance_with_chaos(&mut self, chaos: &ChaosEngram) {
        let mut prng = chaos.seed;
        let threshold = (chaos.energy_level * (u64::MAX as f64)) as u64;

        for block in self.bitmask.iter_mut() {
            // 產生 64-bit 混沌遮罩 (簡單的 XorShift)
            prng ^= prng << 13;
            prng ^= prng >> 7;
            prng ^= prng << 17;

            // 機率過濾：如果 PRNG 低於能量閾值，就翻轉對應的 bit
            let mask = if prng < threshold {
                !0u64 // 全翻轉 (模擬注入高溫雜訊)
            } else {
                prng // 局部翻轉 (混沌擾動)
            };

            *block ^= mask;
        }
    }
}
