use crate::types::*;
use crate::reputation::*;
use std::time::Duration;

/// BranchReputation + DynamicDecay 통합 수식
/// 
/// Trust Score Update Formula:
/// ────────────────────────────────────────────────────────────
/// 
/// trust_new = trust_old * (1 - decay_total) + event_delta
/// 
/// where:
///   decay_total = decay_time + decay_context + decay_hardware
///   
///   decay_time = base_rate * elapsed_days
///   decay_context = decay_time * context_multiplier * I(workload_changed)
///   decay_hardware = decay_time * hardware_multiplier * I(hardware_changed)
///   
///   event_delta = {
///     +0.1  if Success
///     -0.15 if Failure
///     -0.2  if Quarantined
///     +0.1  if ProbationCompleted
///   }
///   
///   I(condition) = indicator function (1 if true, 0 if false)
/// 
/// Constraints:
///   - trust_new ∈ [0, 1]
///   - decay_total ∈ [0, 1]
///   - base_rate = 0.001 (0.1% per day)
///   - context_multiplier = 1.5
///   - hardware_multiplier = 2.0
/// ────────────────────────────────────────────────────────────

/// Reputation update simulator with detailed formula breakdown
pub struct ReputationSimulator {
    base_decay_rate: f64,
    context_multiplier: f64,
    hardware_multiplier: f64,
    history: Vec<SimulationStep>,
}

#[derive(Debug, Clone)]
pub struct SimulationStep {
    pub timestamp: u64,
    pub elapsed_days: f64,
    pub trust_before: f64,
    pub trust_after: f64,
    pub event: ReputationEvent,
    pub decay_breakdown: DecayBreakdown,
    pub formula_trace: FormulaTrace,
}

#[derive(Debug, Clone)]
pub struct DecayBreakdown {
    pub time_component: f64,
    pub context_component: f64,
    pub hardware_component: f64,
    pub total_decay: f64,
}

#[derive(Debug, Clone)]
pub struct FormulaTrace {
    pub expression: String,
    pub values: Vec<(String, f64)>,
    pub result: f64,
}

impl ReputationSimulator {
    pub fn new() -> Self {
        Self {
            base_decay_rate: 0.001,
            context_multiplier: 1.5,
            hardware_multiplier: 2.0,
            history: Vec::new(),
        }
    }

    /// Single update with complete formula trace
    pub fn update_with_trace(
        &mut self,
        reputation: &mut BranchReputation,
        event: ReputationEvent,
        context: &DecayContext,
        elapsed_days: f64,
    ) -> SimulationStep {
        let trust_before = reputation.trust_score;

        // Step 1: Compute decay components
        let decay_time = self.base_decay_rate * elapsed_days;
        
        let decay_context = if context.workload_changed {
            decay_time * self.context_multiplier
        } else {
            0.0
        };

        let decay_hardware = if context.hardware_changed {
            decay_time * self.hardware_multiplier
        } else {
            0.0
        };

        let decay_total = (decay_time + decay_context + decay_hardware).min(1.0);

        // Step 2: Compute event delta
        let event_delta = match event {
            ReputationEvent::Success { .. } => 0.1,
            ReputationEvent::Failure => -0.15,
            ReputationEvent::QuarantineEntry { .. } => -0.2,
            ReputationEvent::ProbationCompleted => 0.1,
        };

        // Step 3: Apply formula
        let trust_after_decay = trust_before * (1.0 - decay_total);
        let trust_new = (trust_after_decay + event_delta).clamp(0.0, 1.0);

        // Step 4: Update reputation
        reputation.trust_score = trust_new;
        match &event {
            ReputationEvent::Success { runtime_ns } => {
                reputation.success_count += 1;
                reputation.total_runtime_ns += runtime_ns;
            }
            ReputationEvent::Failure => {
                reputation.failure_count += 1;
            }
            ReputationEvent::QuarantineEntry { .. } => {}
            ReputationEvent::ProbationCompleted => {
                reputation.probation_completions += 1;
            }
        }
        reputation.last_decay_time = Self::current_timestamp();

        // Step 5: Build trace
        let decay_breakdown = DecayBreakdown {
            time_component: decay_time,
            context_component: decay_context,
            hardware_component: decay_hardware,
            total_decay: decay_total,
        };

        let formula_trace = FormulaTrace {
            expression: format!(
                "trust_new = trust_old * (1 - decay_total) + event_delta\n\
                 decay_total = decay_time + decay_context + decay_hardware\n\
                 trust_new = {} * (1 - {}) + {}\n\
                 trust_new = {} + {}\n\
                 trust_new = {} (clamped to [0, 1])",
                trust_before,
                decay_total,
                event_delta,
                trust_after_decay,
                event_delta,
                trust_new
            ),
            values: vec![
                ("trust_before".to_string(), trust_before),
                ("decay_time".to_string(), decay_time),
                ("decay_context".to_string(), decay_context),
                ("decay_hardware".to_string(), decay_hardware),
                ("decay_total".to_string(), decay_total),
                ("event_delta".to_string(), event_delta),
                ("trust_after_decay".to_string(), trust_after_decay),
                ("trust_new".to_string(), trust_new),
            ],
            result: trust_new,
        };

        let step = SimulationStep {
            timestamp: Self::current_timestamp(),
            elapsed_days,
            trust_before,
            trust_after: trust_new,
            event,
            decay_breakdown,
            formula_trace,
        };

        self.history.push(step.clone());
        step
    }

    /// Run complete simulation scenario
    pub fn simulate_scenario(&mut self, scenario: SimulationScenario) -> SimulationResult {
        let mut reputation = BranchReputation::new();
        reputation.trust_score = scenario.initial_trust;

        let mut steps = Vec::new();

        for event_spec in scenario.events {
            let step = self.update_with_trace(
                &mut reputation,
                event_spec.event,
                &event_spec.context,
                event_spec.elapsed_days,
            );
            steps.push(step);
        }

        SimulationResult {
            initial_trust: scenario.initial_trust,
            final_trust: reputation.trust_score,
            total_steps: steps.len(),
            steps,
            final_reputation: reputation,
        }
    }

    fn current_timestamp() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    pub fn print_step(&self, step: &SimulationStep) {
        println!("\n═══════════════════════════════════════");
        println!("Simulation Step at t={}", step.timestamp);
        println!("═══════════════════════════════════════");
        println!("Event: {:?}", step.event);
        println!("Elapsed days: {:.2}", step.elapsed_days);
        println!("\nDecay Breakdown:");
        println!("  Time component:     {:.6}", step.decay_breakdown.time_component);
        println!("  Context component:  {:.6}", step.decay_breakdown.context_component);
        println!("  Hardware component: {:.6}", step.decay_breakdown.hardware_component);
        println!("  Total decay:        {:.6}", step.decay_breakdown.total_decay);
        println!("\nFormula Trace:");
        println!("{}", step.formula_trace.expression);
        println!("\nValues:");
        for (name, value) in &step.formula_trace.values {
            println!("  {:20} = {:.6}", name, value);
        }
        println!("\nTrust: {:.6} → {:.6}", step.trust_before, step.trust_after);
        println!("═══════════════════════════════════════\n");
    }
}

#[derive(Debug, Clone)]
pub struct SimulationScenario {
    pub name: String,
    pub initial_trust: f64,
    pub events: Vec<EventSpec>,
}

#[derive(Debug, Clone)]
pub struct EventSpec {
    pub event: ReputationEvent,
    pub context: DecayContext,
    pub elapsed_days: f64,
}

#[derive(Debug)]
pub struct SimulationResult {
    pub initial_trust: f64,
    pub final_trust: f64,
    pub total_steps: usize,
    pub steps: Vec<SimulationStep>,
    pub final_reputation: BranchReputation,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_update_formula() {
        let mut simulator = ReputationSimulator::new();
        let mut reputation = BranchReputation::new();
        reputation.trust_score = 0.5;

        let context = DecayContext {
            workload_changed: false,
            hardware_changed: false,
            time_since_last_use: Duration::from_secs(0),
        };

        let step = simulator.update_with_trace(
            &mut reputation,
            ReputationEvent::Success { runtime_ns: 1_000_000 },
            &context,
            1.0, // 1 day elapsed
        );

        // Verify decay calculation
        // decay_time = 0.001 * 1.0 = 0.001
        // decay_total = 0.001 (no context/hardware change)
        assert!((step.decay_breakdown.time_component - 0.001).abs() < 1e-6);
        assert_eq!(step.decay_breakdown.context_component, 0.0);
        assert_eq!(step.decay_breakdown.hardware_component, 0.0);
        assert!((step.decay_breakdown.total_decay - 0.001).abs() < 1e-6);

        // Verify trust update
        // trust_new = 0.5 * (1 - 0.001) + 0.1 = 0.4995 + 0.1 = 0.5995
        let expected = (0.5 * 0.999 + 0.1).min(1.0);
        assert!((reputation.trust_score - expected).abs() < 1e-6);

        simulator.print_step(&step);
    }

    #[test]
    fn test_context_drift_multiplier() {
        let mut simulator = ReputationSimulator::new();
        let mut reputation = BranchReputation::new();
        reputation.trust_score = 0.8;

        let context = DecayContext {
            workload_changed: true, // Context drift
            hardware_changed: false,
            time_since_last_use: Duration::from_secs(0),
        };

        let step = simulator.update_with_trace(
            &mut reputation,
            ReputationEvent::Success { runtime_ns: 1_000_000 },
            &context,
            10.0, // 10 days
        );

        // decay_time = 0.001 * 10 = 0.01
        // decay_context = 0.01 * 1.5 = 0.015 (context changed)
        // decay_total = 0.01 + 0.015 = 0.025
        assert!((step.decay_breakdown.time_component - 0.01).abs() < 1e-6);
        assert!((step.decay_breakdown.context_component - 0.015).abs() < 1e-6);
        assert!((step.decay_breakdown.total_decay - 0.025).abs() < 1e-6);

        // trust_new = 0.8 * (1 - 0.025) + 0.1 = 0.78 + 0.1 = 0.88
        assert!((reputation.trust_score - 0.88).abs() < 1e-6);
    }

    #[test]
    fn test_hardware_shift_multiplier() {
        let mut simulator = ReputationSimulator::new();
        let mut reputation = BranchReputation::new();
        reputation.trust_score = 0.7;

        let context = DecayContext {
            workload_changed: false,
            hardware_changed: true, // Hardware shift
            time_since_last_use: Duration::from_secs(0),
        };

        let step = simulator.update_with_trace(
            &mut reputation,
            ReputationEvent::Failure,
            &context,
            5.0, // 5 days
        );

        // decay_time = 0.001 * 5 = 0.005
        // decay_hardware = 0.005 * 2.0 = 0.01 (hardware changed)
        // decay_total = 0.005 + 0.01 = 0.015
        assert!((step.decay_breakdown.hardware_component - 0.01).abs() < 1e-6);

        // trust_new = 0.7 * (1 - 0.015) - 0.15 = 0.6895 - 0.15 = 0.5395
        let expected = (0.7 * 0.985 - 0.15).max(0.0);
        assert!((reputation.trust_score - expected).abs() < 1e-6);
    }

    #[test]
    fn test_complete_lifecycle_simulation() {
        let mut simulator = ReputationSimulator::new();

        let scenario = SimulationScenario {
            name: "Branch Lifecycle".to_string(),
            initial_trust: 0.5,
            events: vec![
                // Day 1: Success
                EventSpec {
                    event: ReputationEvent::Success { runtime_ns: 1_000_000 },
                    context: DecayContext {
                        workload_changed: false,
                        hardware_changed: false,
                        time_since_last_use: Duration::from_secs(0),
                    },
                    elapsed_days: 1.0,
                },
                // Day 5: Another success
                EventSpec {
                    event: ReputationEvent::Success { runtime_ns: 1_000_000 },
                    context: DecayContext {
                        workload_changed: false,
                        hardware_changed: false,
                        time_since_last_use: Duration::from_secs(0),
                    },
                    elapsed_days: 4.0,
                },
                // Day 10: Failure
                EventSpec {
                    event: ReputationEvent::Failure,
                    context: DecayContext {
                        workload_changed: false,
                        hardware_changed: false,
                        time_since_last_use: Duration::from_secs(0),
                    },
                    elapsed_days: 5.0,
                },
                // Day 15: Quarantine
                EventSpec {
                    event: ReputationEvent::QuarantineEntry {
                        reason: QuarantineReason::PerformanceAnomaly { spike_magnitude: 5.0 },
                    },
                    context: DecayContext {
                        workload_changed: true, // Workload changed
                        hardware_changed: false,
                        time_since_last_use: Duration::from_secs(0),
                    },
                    elapsed_days: 5.0,
                },
                // Day 20: Probation completed
                EventSpec {
                    event: ReputationEvent::ProbationCompleted,
                    context: DecayContext {
                        workload_changed: false,
                        hardware_changed: false,
                        time_since_last_use: Duration::from_secs(0),
                    },
                    elapsed_days: 5.0,
                },
            ],
        };

        let result = simulator.simulate_scenario(scenario);

        println!("\n╔═══════════════════════════════════════════╗");
        println!("║   COMPLETE LIFECYCLE SIMULATION RESULT   ║");
        println!("╚═══════════════════════════════════════════╝");
        println!("Initial trust:  {:.6}", result.initial_trust);
        println!("Final trust:    {:.6}", result.final_trust);
        println!("Total steps:    {}", result.total_steps);
        println!("Success count:  {}", result.final_reputation.success_count);
        println!("Failure count:  {}", result.final_reputation.failure_count);
        println!("Probation completions: {}", result.final_reputation.probation_completions);

        // Verify trust trend
        assert_eq!(result.total_steps, 5);
        assert!(result.final_reputation.success_count == 2);
        assert!(result.final_reputation.failure_count == 1);

        // Print each step
        for step in &result.steps {
            simulator.print_step(step);
        }
    }

    #[test]
    fn test_extreme_decay_scenario() {
        let mut simulator = ReputationSimulator::new();
        let mut reputation = BranchReputation::new();
        reputation.trust_score = 1.0;

        let context = DecayContext {
            workload_changed: true,
            hardware_changed: true,
            time_since_last_use: Duration::from_secs(0),
        };

        // Simulate 100 days of inactivity with both context and hardware changes
        let step = simulator.update_with_trace(
            &mut reputation,
            ReputationEvent::Failure,
            &context,
            100.0,
        );

        // decay_time = 0.001 * 100 = 0.1
        // decay_context = 0.1 * 1.5 = 0.15
        // decay_hardware = 0.1 * 2.0 = 0.2
        // decay_total = 0.1 + 0.15 + 0.2 = 0.45
        assert!((step.decay_breakdown.total_decay - 0.45).abs() < 1e-6);

        // trust_new = 1.0 * (1 - 0.45) - 0.15 = 0.55 - 0.15 = 0.40
        assert!((reputation.trust_score - 0.40).abs() < 1e-6);

        simulator.print_step(&step);
    }

    #[test]
    fn test_trust_bounds() {
        let mut simulator = ReputationSimulator::new();
        
        // Test lower bound
        let mut reputation = BranchReputation::new();
        reputation.trust_score = 0.05;

        let context = DecayContext {
            workload_changed: false,
            hardware_changed: false,
            time_since_last_use: Duration::from_secs(0),
        };

        simulator.update_with_trace(
            &mut reputation,
            ReputationEvent::Failure,
            &context,
            1.0,
        );

        // Should clamp to 0.0
        assert!(reputation.trust_score >= 0.0);

        // Test upper bound
        let mut reputation = BranchReputation::new();
        reputation.trust_score = 0.95;

        for _ in 0..10 {
            simulator.update_with_trace(
                &mut reputation,
                ReputationEvent::Success { runtime_ns: 1_000_000 },
                &context,
                0.1,
            );
        }

        // Should clamp to 1.0
        assert!(reputation.trust_score <= 1.0);
    }
}
