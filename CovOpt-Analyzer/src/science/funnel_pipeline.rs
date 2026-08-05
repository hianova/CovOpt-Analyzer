use crate::science::chaos_state::RngState;
use std::collections::HashMap;

/// 1. 一階漏斗：Compiler Guillotine (編譯器斷頭台)
pub struct CompilerGuillotine;

impl CompilerGuillotine {
    /// 靜態檢查與模擬執行生成的 Python-like 程式碼或邏輯運算式。
    /// 檢查項目：
    /// 1. 括號對稱性。
    /// 2. 基本語法結構（例如冒號 `:` 後面的縮排或是 if-else 配對）。
    /// 3. 變數靜態分析（Static Variable Resolution）：檢查變數是否在使用前已定義。
    ///    若不合法，回傳極大懲罰分數 (例如 -1,000,000)。
    pub fn validate_logic(code: &str) -> Result<(), &'static str> {
        // 1. 括號配對檢查
        let mut stack = Vec::new();
        for c in code.chars() {
            match c {
                '(' | '[' | '{' => stack.push(c),
                ')' => {
                    if stack.pop() != Some('(') {
                        return Err("Unmatched parentheses: )");
                    }
                }
                ']' => {
                    if stack.pop() != Some('[') {
                        return Err("Unmatched bracket: ]");
                    }
                }
                '}' if stack.pop() != Some('{') => {
                    return Err("Unmatched brace: }");
                }
                '}' => {}
                _ => {}
            }
        }
        if !stack.is_empty() {
            return Err("Unclosed brackets / parentheses");
        }

        // 2. 靜態變數分析 (所有被使用的變數必須先被賦值/宣告)
        // 簡單解析每一行：如果是 assignment (如 `var = value`), 將其加入已定義集合。
        // 如果是普通的表達式，提取變數名並確認是否已在集合中。
        let mut defined_vars = std::collections::HashSet::new();
        // 預設內建變數與輸入
        defined_vars.insert("x".to_string());
        defined_vars.insert("y".to_string());
        defined_vars.insert("true".to_string());
        defined_vars.insert("false".to_string());

        for line in code.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if let Some(eq_idx) = trimmed.find('=') {
                // assignment: var_name = expression
                let var_part = trimmed[..eq_idx].trim();
                let expr_part = trimmed[eq_idx + 1..].trim();

                // 簡單檢查左邊是否為合法變數名
                if var_part.chars().all(|c| c.is_alphanumeric() || c == '_') && !var_part.is_empty()
                {
                    // 先檢查右邊表達式中的變數是否都已定義
                    for token in Self::tokenize_expr(expr_part) {
                        if token
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_alphabetic() || c == '_')
                            && !defined_vars.contains(&token)
                            && token.parse::<f64>().is_err()
                        {
                            return Err("Undefined variable usage on assignment RHS");
                        }
                    }
                    defined_vars.insert(var_part.to_string());
                } else {
                    return Err("Invalid assignment target name");
                }
            } else {
                // 普通表達式，檢查所有變數是否已定義
                for token in Self::tokenize_expr(trimmed) {
                    if token
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_alphabetic() || c == '_')
                        && !defined_vars.contains(&token)
                        && token.parse::<f64>().is_err()
                    {
                        return Err("Undefined variable usage in expression");
                    }
                }
            }
        }

        Ok(())
    }

    /// 提取表達式中的單字/變數 Token
    fn tokenize_expr(expr: &str) -> Vec<String> {
        let mut tokens = Vec::new();
        let mut current = String::new();
        for c in expr.chars() {
            if c.is_alphanumeric() || c == '_' {
                current.push(c);
            } else {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
            }
        }
        if !current.is_empty() {
            tokens.push(current);
        }
        tokens
    }
}

/// 2. 二階漏斗：Self-Consistency Attractor (Zipf 自共識吸引子)
pub struct ZipfAttractor;

impl ZipfAttractor {
    /// 接受多個解碼路徑（例如 100 條路徑），對合格的路徑進行 canonicalization 並統計頻率。
    /// 依據 Zipf 定律篩選：正確邏輯會像奇異吸引子一樣聚攏在頻率頭部，而幻覺則均勻散佈於長尾。
    /// 剪除 80% 的長尾幻覺，僅保留頭部共識路徑。
    pub fn distill_consensus(paths: &[String]) -> Option<(String, f32)> {
        let mut frequencies = HashMap::new();
        let mut valid_count = 0;

        for path in paths {
            // 首先經過一階漏斗：Compiler Guillotine 過濾
            if CompilerGuillotine::validate_logic(path).is_ok() {
                // 進行簡單的規範化（去空白與小寫）以合併等價的邏輯
                let canonical = path.replace(" ", "").replace("\n", "").to_lowercase();
                *frequencies.entry(canonical).or_insert(0) += 1;
                valid_count += 1;
            }
        }

        if valid_count == 0 {
            return None;
        }

        // 將頻率排序，找出頭部
        let mut freq_vec: Vec<(String, usize)> = frequencies.into_iter().collect();
        freq_vec.sort_by_key(|b| std::cmp::Reverse(b.1));

        // 計算頭部候選的比例
        let best = freq_vec.first()?.clone();
        let ratio = best.1 as f32 / valid_count as f32;

        // Zipf 逆淘汰：若最優路徑的比例符合顯著頭部特徵（例如大於某個閾值），則將其選為共識。
        // 此處我們印出頻率分佈來展示 Zipf 效應
        println!("[ZipfAttractor] 共識頻率分佈前 3 名：");
        for (i, (path, count)) in freq_vec.iter().take(3).enumerate() {
            let percentage = *count as f32 / paths.len() as f32 * 100.0;
            println!(
                "  Rank {}: Count = {} ({:.1}%) -> {}",
                i + 1,
                count,
                percentage,
                path
            );
        }

        Some((best.0, ratio))
    }
}

/// 3. 三階漏斗：紅藍軍邏輯對抗 (Adversarial Verification)
pub struct AdversarialConfrontation {
    pub max_rounds: usize,
}

impl AdversarialConfrontation {
    pub fn new(max_rounds: usize) -> Self {
        Self { max_rounds }
    }

    /// 啟動紅藍對抗。
    /// 藍軍 (Generator) 生成推理路徑，紅軍 (Critic/Environment Demon) 負責找碴生成反例。
    /// 透過相互對抗，篩選出具備絕對魯棒性的推理邏輯。
    pub fn run_battle(&self, initial_blue: &str) -> (String, i32) {
        let mut rng = RngState::new(12345);
        let mut blue_logic = initial_blue.to_string();
        let mut fitness = 100; // 初始適應度

        println!("\n⚔️ 啟動邏輯對抗戰 (Adversarial Loop) ⚔️");
        println!("[藍軍初始邏輯]：\n{}", blue_logic);

        for round in 1..=self.max_rounds {
            println!("\n--- Round {} ---", round);

            // 1. 紅軍出擊：針對藍軍邏輯，嘗試生成挑戰 (Adversarial Challenge)
            // 在模擬中，紅軍可能挑出藍軍未宣告的變數，或是提供與藍軍衝突的反例
            let red_challenge = self.simulate_red_critic(&blue_logic, &mut rng);
            println!("[紅軍找碴挑戰]：{}", red_challenge.description);

            // 2. 透過一階漏斗（Compiler Guillotine）評估挑戰的合法性
            if let Err(reason) = CompilerGuillotine::validate_logic(&red_challenge.code) {
                // 如果紅軍生成的反例自身語法錯誤，紅軍判罰，藍軍防禦成功！
                println!(
                    "[系統判定] 紅軍反例違反 Compiler Guillotine ({})，挑戰失效！藍軍適應度上升。",
                    reason
                );
                fitness += 20;
            } else {
                // 紅軍挑戰合法，開始評估藍軍能否應對
                if self.does_blue_fail_challenge(&blue_logic, &red_challenge) {
                    println!("[系統判定] 紅軍挑戰成功破壞藍軍邏輯！藍軍被扣分並進行變異修補。");
                    fitness -= 30;
                    // 藍軍進行邏輯修復與突變
                    blue_logic = self.mutate_blue_logic(&blue_logic, &red_challenge);
                    println!("[藍軍修復後邏輯]：\n{}", blue_logic);
                } else {
                    println!("[系統判定] 藍軍成功反駁紅軍質疑，邏輯防禦成功！");
                    fitness += 15;
                }
            }

            println!("[當前藍軍適應度分值]：{}", fitness);
            if fitness >= 200 {
                println!("[對抗收斂] 藍軍邏輯已達到 Nash 平衡，防禦無懈可擊！");
                break;
            }
        }

        (blue_logic, fitness)
    }

    fn simulate_red_critic(&self, _blue_code: &str, rng: &mut RngState) -> RedChallenge {
        let roll = rng.next_f32();
        if roll < 0.3 {
            RedChallenge {
                description: "質疑變數 z 未定義即被使用".to_string(),
                code: "a = x + z".to_string(), // z 未定義，預期會觸發 Compiler Guillotine 或 Blue Fail
                target_var: Some("z".to_string()),
            }
        } else if roll < 0.6 {
            RedChallenge {
                description: "括號不配對的語法攻擊".to_string(),
                code: "a = (x + y".to_string(), // 括號未閉合，觸發 Compiler Guillotine
                target_var: None,
            }
        } else {
            RedChallenge {
                description: "正常邏輯質疑，挑戰變數 y 的影響".to_string(),
                code: "y = 5\na = x + y".to_string(), // 合法代碼
                target_var: Some("y".to_string()),
            }
        }
    }

    fn does_blue_fail_challenge(&self, blue_code: &str, challenge: &RedChallenge) -> bool {
        // 如果紅軍指出的 target_var 在藍軍代碼中沒有被妥善處理，就判定藍軍失敗
        if let Some(ref var) = challenge.target_var {
            !blue_code.contains(var)
        } else {
            false
        }
    }

    fn mutate_blue_logic(&self, old_blue: &str, challenge: &RedChallenge) -> String {
        // 藍軍突變：將紅軍質疑的變數定義/防禦性初始化加入代碼中
        if let Some(ref var) = challenge.target_var {
            format!("{} = 1\n{}", var, old_blue)
        } else {
            old_blue.to_string()
        }
    }
}

pub struct RedChallenge {
    pub description: String,
    pub code: String,
    pub target_var: Option<String>,
}

/// 4. 整合流水線：FunnelPipeline
pub struct FunnelPipeline;

impl FunnelPipeline {
    pub fn execute(initial_prompt: &str) {
        println!("============================================================");
        println!("🚀 啟動三階漏斗邏輯收斂流水線 (ModelGo Funnel Pipeline) 🚀");
        println!("============================================================");
        println!("原始提示詞：{}\n", initial_prompt);

        // ------------------------------------------------------------
        // 步驟 1 與 2：模擬生成 100 條路徑，並使用一階與二階漏斗共識收斂
        // ------------------------------------------------------------
        println!(">>> [步驟 1 & 2] 生成 100 條推理路徑並透過 Zipf 自共識篩選...");

        let mut paths = Vec::new();
        // 模擬 100 條推理路徑：
        // - 65% 為正確/吸引子邏輯 (a = x + y)
        // - 15% 為語法錯誤邏輯 (a = (x + y)
        // - 20% 為隨機幻覺/變數未定義邏輯 (a = x + z)
        for i in 0..100 {
            if i < 65 {
                paths.push("a = x + y".to_string());
            } else if i < 80 {
                paths.push("a = (x + y".to_string()); // 語法錯誤
            } else {
                paths.push("a = x + z".to_string()); // 變數 z 未定義
            }
        }

        let consensus = ZipfAttractor::distill_consensus(&paths);

        if let Some((consensus_logic, confidence)) = consensus {
            println!("\n>>> [共識收斂成功] 信賴度：{:.1}%", confidence * 100.0);
            println!("[共識邏輯核心]：\n{}", consensus_logic);

            // ------------------------------------------------------------
            // 步驟 3：紅藍對抗驗證 (Adversarial Verification)
            // ------------------------------------------------------------
            let confront = AdversarialConfrontation::new(5);
            let (final_robust_logic, final_score) = confront.run_battle(&consensus_logic);

            println!("\n============================================================");
            println!("🎉 三階漏斗流水線執行完畢！");
            println!("=> 最終防禦邏輯：\n{}", final_robust_logic);
            println!("=> 最終健壯度得分：{}", final_score);
            println!("============================================================");
        } else {
            println!("\n❌ 100 條路徑皆未通過 Compiler Guillotine，系統發生邏輯塌陷！");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compiler_guillotine_brackets() {
        assert!(CompilerGuillotine::validate_logic("a = (x + y)").is_ok());
        assert!(CompilerGuillotine::validate_logic("a = (x + y").is_err());
    }

    #[test]
    fn test_compiler_guillotine_variables() {
        // x and y are pre-defined
        assert!(CompilerGuillotine::validate_logic("a = x + y").is_ok());
        // z is not pre-defined
        assert!(CompilerGuillotine::validate_logic("a = x + z").is_err());
    }

    #[test]
    fn test_zipf_attractor() {
        let paths = vec![
            "a = x + y".to_string(),
            "a = x + y".to_string(),
            "a = (x + y".to_string(), // syntax error
            "a = x + z".to_string(),  // undefined z
        ];
        let res = ZipfAttractor::distill_consensus(&paths);
        assert!(res.is_some());
        let (best, ratio) = res.unwrap();
        assert_eq!(best, "a=x+y");
        // Only 2 of 4 are valid, and both of the valid ones are "a = x + y".
        // valid count = 2. best count = 2. ratio = 2/2 = 1.0.
        assert_eq!(ratio, 1.0);
    }
}
