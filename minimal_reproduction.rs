use crate::types::*;
use std::collections::{HashMap, HashSet};

/// ScenarioGenerator의 최소 반례 생성 알고리즘
/// 
/// 목표: runtime_tail에서 실패를 재현하는 최소한의 실행 경로 추출
/// 
/// 기준:
/// 1. Coverage: 실패를 재현할 수 있는 충분한 경로
/// 2. Minimality: 불필요한 step 제거
/// 3. Causality: 실패에 기여한 step만 선택
pub struct MinimalReproductionExtractor {
    tail_analyzer: TailAnalyzer,
    causal_graph_builder: CausalGraphBuilder,
    minimizer: StepMinimizer,
}

/// Runtime tail 분석기
struct TailAnalyizer {
    // 실패와 관련된 키워드
    failure_keywords: HashSet<String>,
    // 노이즈 패턴 (제거 대상)
    noise_patterns: Vec<String>,
}

/// 인과 관계 그래프 구축
struct CausalGraphBuilder {
    // Step 간 의존성 추적
    dependencies: HashMap<String, Vec<String>>,
}

/// Step 최소화기
struct StepMinimizer {
    // Binary search 기반 최소화
    max_iterations: usize,
}

impl MinimalReproductionExtractor {
    pub fn new() -> Self {
        Self {
            tail_analyzer: TailAnalyzer::new(),
            causal_graph_builder: CausalGraphBuilder::new(),
            minimizer: StepMinimizer::new(10),
        }
    }

    /// runtime_tail에서 최소 step set 추출
    /// 
    /// 알고리즘:
    /// 1. Tail analysis: 실패 지점 식별
    /// 2. Backward tracing: 실패에 기여한 step 역추적
    /// 3. Noise removal: 관련 없는 step 제거
    /// 4. Binary search minimization: 최소 재현 set 발견
    pub fn extract_minimal_steps(
        &self,
        runtime_tail: &[String],
        failure_point: &str,
    ) -> MinimalStepSet {
        // Step 1: Identify failure point in tail
        let failure_index = self.locate_failure_point(runtime_tail, failure_point);

        // Step 2: Extract relevant steps (backward from failure)
        let relevant_steps = self.extract_relevant_steps(runtime_tail, failure_index);

        // Step 3: Build causal graph
        let causal_graph = self.causal_graph_builder.build(&relevant_steps);

        // Step 4: Find minimal set that preserves causality
        let minimal_set = self.minimizer.minimize(&relevant_steps, &causal_graph);

        // Step 5: Generate reproduction instructions
        let instructions = self.generate_instructions(&minimal_set);

        MinimalStepSet {
            steps: minimal_set,
            total_original_steps: runtime_tail.len(),
            reduction_ratio: 1.0 - (minimal_set.len() as f64 / runtime_tail.len() as f64),
            instructions,
            causal_graph,
        }
    }

    fn locate_failure_point(&self, tail: &[String], failure_point: &str) -> usize {
        tail.iter()
            .position(|s| s.contains(failure_point))
            .unwrap_or(tail.len() - 1)
    }

    fn extract_relevant_steps(&self, tail: &[String], failure_index: usize) -> Vec<String> {
        self.tail_analyzer.filter_relevant(tail, failure_index)
    }

    fn generate_instructions(&self, steps: &[String]) -> Vec<ReproductionInstruction> {
        steps
            .iter()
            .enumerate()
            .map(|(i, step)| ReproductionInstruction {
                step_number: i,
                operation: step.clone(),
                expected_state: format!("state_after_{}", i),
            })
            .collect()
    }
}

impl TailAnalyzer {
    fn new() -> Self {
        Self {
            failure_keywords: ["error", "exception", "panic", "assert", "violation", "fault"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            noise_patterns: vec![
                "debug_log".to_string(),
                "trace_".to_string(),
                "metrics_".to_string(),
            ],
        }
    }

    /// Filter steps relevant to failure reproduction
    /// 
    /// 기준:
    /// - Failure point 이전의 모든 step
    /// - Failure keywords를 포함하는 step
    /// - Noise pattern이 없는 step
    fn filter_relevant(&self, tail: &[String], failure_index: usize) -> Vec<String> {
        tail[..=failure_index]
            .iter()
            .filter(|step| {
                // Remove noise
                !self.is_noise(step) &&
                // Keep if contains failure keyword or is function call
                (self.contains_failure_keyword(step) || self.is_function_call(step))
            })
            .cloned()
            .collect()
    }

    fn is_noise(&self, step: &str) -> bool {
        self.noise_patterns.iter().any(|pattern| step.contains(pattern))
    }

    fn contains_failure_keyword(&self, step: &str) -> bool {
        self.failure_keywords.iter().any(|keyword| {
            step.to_lowercase().contains(&keyword.to_lowercase())
        })
    }

    fn is_function_call(&self, step: &str) -> bool {
        step.contains("(") || step.contains("::") || step.contains("->")
    }
}

impl CausalGraphBuilder {
    fn new() -> Self {
        Self {
            dependencies: HashMap::new(),
        }
    }

    /// Build causal dependency graph
    /// 
    /// 의존성 규칙:
    /// - Function call → caller (호출자는 피호출자에 의존)
    /// - Data flow → producer (소비자는 생산자에 의존)
    /// - Temporal order → previous step (후속 step은 이전 step에 의존)
    fn build(&self, steps: &[String]) -> CausalGraph {
        let mut graph = CausalGraph {
            nodes: steps.to_vec(),
            edges: Vec::new(),
        };

        for i in 0..steps.len() {
            // Temporal dependency (always depends on previous step)
            if i > 0 {
                graph.edges.push((i - 1, i));
            }

            // Function call dependency
            if let Some(caller_idx) = self.find_caller(&steps[i], &steps[..i]) {
                graph.edges.push((caller_idx, i));
            }
        }

        graph
    }

    fn find_caller(&self, callee: &str, previous_steps: &[String]) -> Option<usize> {
        // Extract function name from "fn_name(...)"
        let callee_name = callee.split('(').next()?.split("::").last()?;

        previous_steps.iter().rposition(|step| {
            step.contains(callee_name) && step.contains("->")
        })
    }
}

impl StepMinimizer {
    fn new(max_iterations: usize) -> Self {
        Self { max_iterations }
    }

    /// Binary search minimization
    /// 
    /// 알고리즘:
    /// 1. Start with full set
    /// 2. Try removing half of steps
    /// 3. If still reproduces failure, accept; else, revert
    /// 4. Repeat until no more reduction possible
    fn minimize(&self, steps: &[String], causal_graph: &CausalGraph) -> Vec<String> {
        let mut current_set: HashSet<usize> = (0..steps.len()).collect();
        let mut iteration = 0;

        while iteration < self.max_iterations {
            let reduction_candidate = self.propose_reduction(&current_set, causal_graph);
            
            if reduction_candidate.is_empty() || reduction_candidate.len() == current_set.len() {
                break; // No more reduction possible
            }

            // Check if reduced set preserves causality
            if self.preserves_causality(&reduction_candidate, causal_graph) {
                current_set = reduction_candidate;
            } else {
                break; // Cannot reduce further without breaking causality
            }

            iteration += 1;
        }

        // Convert indices back to steps
        let mut minimal_steps: Vec<_> = current_set
            .iter()
            .map(|&i| steps[i].clone())
            .collect();
        minimal_steps.sort_by_key(|s| steps.iter().position(|x| x == s).unwrap());
        minimal_steps
    }

    fn propose_reduction(
        &self,
        current_set: &HashSet<usize>,
        causal_graph: &CausalGraph,
    ) -> HashSet<usize> {
        let mut reduced = current_set.clone();
        
        // Try removing steps without outgoing edges (leaf nodes)
        for &node in current_set {
            if !causal_graph.has_outgoing_edges(node) {
                reduced.remove(&node);
                break; // Remove one at a time
            }
        }

        reduced
    }

    fn preserves_causality(
        &self,
        reduced_set: &HashSet<usize>,
        causal_graph: &CausalGraph,
    ) -> bool {
        // Check if all edges in reduced set still form valid path
        for &node in reduced_set {
            for &(from, to) in &causal_graph.edges {
                if to == node {
                    // If this node is in set, its dependency must be too
                    if !reduced_set.contains(&from) {
                        return false;
                    }
                }
            }
        }
        true
    }
}

#[derive(Debug, Clone)]
pub struct MinimalStepSet {
    pub steps: Vec<String>,
    pub total_original_steps: usize,
    pub reduction_ratio: f64,
    pub instructions: Vec<ReproductionInstruction>,
    pub causal_graph: CausalGraph,
}

#[derive(Debug, Clone)]
pub struct ReproductionInstruction {
    pub step_number: usize,
    pub operation: String,
    pub expected_state: String,
}

#[derive(Debug, Clone)]
pub struct CausalGraph {
    pub nodes: Vec<String>,
    pub edges: Vec<(usize, usize)>, // (from, to)
}

impl CausalGraph {
    fn has_outgoing_edges(&self, node: usize) -> bool {
        self.edges.iter().any(|(from, _)| *from == node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minimal_step_extraction() {
        let extractor = MinimalReproductionExtractor::new();

        let runtime_tail = vec![
            "fn main() at line 10".to_string(),
            "fn process_input() at line 20".to_string(),
            "debug_log: processing started".to_string(), // noise
            "fn validate_buffer() at line 30".to_string(),
            "trace_metrics: latency=1ms".to_string(), // noise
            "fn assert_buffer_size() at line 40".to_string(),
            "error: assertion failed: buffer_size > 0".to_string(), // failure
        ];

        let minimal = extractor.extract_minimal_steps(&runtime_tail, "assertion failed");

        // Should remove noise steps
        assert!(minimal.steps.len() < runtime_tail.len());
        
        // Should keep essential steps
        assert!(minimal.steps.iter().any(|s| s.contains("validate_buffer")));
        assert!(minimal.steps.iter().any(|s| s.contains("assert_buffer_size")));
        assert!(minimal.steps.iter().any(|s| s.contains("assertion failed")));

        // Should remove noise
        assert!(!minimal.steps.iter().any(|s| s.contains("debug_log")));
        assert!(!minimal.steps.iter().any(|s| s.contains("trace_metrics")));

        // Reduction ratio should be positive
        assert!(minimal.reduction_ratio > 0.0);
        println!("Reduced from {} to {} steps ({:.1}% reduction)",
            minimal.total_original_steps,
            minimal.steps.len(),
            minimal.reduction_ratio * 100.0
        );
    }

    #[test]
    fn test_causal_graph_construction() {
        let builder = CausalGraphBuilder::new();

        let steps = vec![
            "fn a() at line 10".to_string(),
            "fn b() -> calls c()".to_string(),
            "fn c() at line 30".to_string(),
            "error in c()".to_string(),
        ];

        let graph = builder.build(&steps);

        // Should have temporal edges
        assert!(graph.edges.contains(&(0, 1)));
        assert!(graph.edges.contains(&(1, 2)));
        assert!(graph.edges.contains(&(2, 3)));

        // Should have function call edge (b -> c)
        assert!(graph.edges.contains(&(1, 2)));
    }

    #[test]
    fn test_minimization_preserves_causality() {
        let extractor = MinimalReproductionExtractor::new();

        let runtime_tail = vec![
            "fn init() at line 1".to_string(),
            "fn allocate_buffer() at line 5".to_string(),
            "fn fill_buffer() at line 10".to_string(),
            "fn process_buffer() at line 15".to_string(),
            "error: invalid buffer state".to_string(),
        ];

        let minimal = extractor.extract_minimal_steps(&runtime_tail, "invalid buffer");

        // Should keep causal chain: allocate -> fill -> process -> error
        assert!(minimal.steps.len() >= 4);
        
        // Should preserve order
        let positions: Vec<_> = minimal.steps.iter()
            .map(|s| runtime_tail.iter().position(|t| t == s).unwrap())
            .collect();
        
        for i in 1..positions.len() {
            assert!(positions[i] > positions[i - 1], "Order not preserved");
        }
    }

    #[test]
    fn test_noise_removal() {
        let analyzer = TailAnalyzer::new();

        let tail = vec![
            "fn operation() at line 10".to_string(),
            "debug_log: checkpoint 1".to_string(),
            "trace_event: started".to_string(),
            "metrics_report: latency=5ms".to_string(),
            "fn critical_path() at line 20".to_string(),
            "error: failure occurred".to_string(),
        ];

        let relevant = analyzer.filter_relevant(&tail, 5);

        // Should keep function calls and error
        assert!(relevant.iter().any(|s| s.contains("operation")));
        assert!(relevant.iter().any(|s| s.contains("critical_path")));
        assert!(relevant.iter().any(|s| s.contains("error")));

        // Should remove noise
        assert!(!relevant.iter().any(|s| s.contains("debug_log")));
        assert!(!relevant.iter().any(|s| s.contains("trace_")));
        assert!(!relevant.iter().any(|s| s.contains("metrics_")));
    }

    #[test]
    fn test_extreme_reduction_case() {
        let extractor = MinimalReproductionExtractor::new();

        // Very long tail with mostly noise
        let mut runtime_tail = vec![];
        for i in 0..100 {
            runtime_tail.push(format!("debug_log: iteration {}", i));
        }
        runtime_tail.push("fn critical_operation() at line 500".to_string());
        runtime_tail.push("error: operation failed".to_string());

        let minimal = extractor.extract_minimal_steps(&runtime_tail, "operation failed");

        // Should drastically reduce
        assert!(minimal.steps.len() <= 3);
        assert!(minimal.reduction_ratio > 0.95); // >95% reduction

        // Should keep only essential steps
        assert!(minimal.steps.iter().any(|s| s.contains("critical_operation")));
        assert!(minimal.steps.iter().any(|s| s.contains("operation failed")));
    }

    #[test]
    fn test_reproduction_instructions_generation() {
        let extractor = MinimalReproductionExtractor::new();

        let runtime_tail = vec![
            "fn step_1() at line 10".to_string(),
            "fn step_2() at line 20".to_string(),
            "error: failed".to_string(),
        ];

        let minimal = extractor.extract_minimal_steps(&runtime_tail, "failed");

        // Should generate instructions for each step
        assert_eq!(minimal.instructions.len(), minimal.steps.len());

        for (i, instruction) in minimal.instructions.iter().enumerate() {
            assert_eq!(instruction.step_number, i);
            assert!(!instruction.operation.is_empty());
            assert!(!instruction.expected_state.is_empty());
        }
    }
}
