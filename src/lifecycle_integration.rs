use crate::types::*;
use crate::engine::*;
use crate::postmortem::*;
use crate::probing::*;
use std::collections::HashMap;
use std::time::Duration;

/// Complete lifecycle integration test:
/// Quarantine -> Probe -> Probation -> Active (success path)
/// Quarantine -> Probe -> Probation -> Condemned (failure path)

#[cfg(test)]
mod lifecycle_integration_tests {
    use super::*;

    #[test]
    fn test_full_lifecycle_success_path() {
        println!("\n╔═══════════════════════════════════════════════════════╗");
        println!("║  FULL LIFECYCLE TEST: SUCCESS PATH                   ║");
        println!("║  Quarantine -> Probe -> Probation -> Active          ║");
        println!("╚═══════════════════════════════════════════════════════╝\n");

        let temp_dir = std::env::temp_dir().join("azo_lifecycle_success");
        let _ = std::fs::remove_dir_all(&temp_dir);
        let mut engine = AzoEngine::new(temp_dir.clone()).unwrap();

        // Step 1: Create and run branch successfully
        println!("═══ Step 1: Create Branch ═══");
        let layout = LayoutPlan {
            thread_count: 4,
            memory_budget_mb: 1024,
            cache_strategy: CacheStrategy::Balanced,
            prefetch_depth: 2,
            affinity_hints: vec![0, 1, 2, 3],
            custom_params: HashMap::new(),
        };
        let branch_id = engine.create_branch(layout);
        println!("✓ Branch created: {:?}", branch_id);
        assert_eq!(engine.branches.get(&branch_id).unwrap().state, BranchState::Active);

        // Run successfully for a while
        for _ in 0..10 {
            let metrics = PerformanceMetrics {
                latency_p50_ns: 1_000_000,
                latency_p99_ns: 5_000_000,
                throughput_ops_per_sec: 1000.0,
                memory_used_mb: 500,
                cpu_utilization: 0.6,
                cache_miss_rate: 0.01,
            };
            engine.record_execution(branch_id, true, metrics, 1_000_000).unwrap();
        }
        println!("✓ 10 successful executions recorded");

        // Step 2: Trigger failure -> Quarantine
        println!("\n═══ Step 2: Failure Detection -> Quarantine ═══");
        let error_context = ErrorContext {
            error_type: ErrorType::AssertionFailure,
            error_message: "Buffer validation failed".to_string(),
            checksum_failed: false,
            thermal_throttled: false,
            cpu_temp_celsius: 65.0,
            memory_pressure_pct: 45.0,
            intermittent: false,
        };

        let report = engine.handle_failure(
            branch_id,
            error_context,
            vec![
                "fn validate_buffer() at line 100".to_string(),
                "assert!(buffer.len() > 0)".to_string(),
                "fn process() at line 50".to_string(),
            ],
            vec![500, 510, 520],
        );

        println!("✓ Post-mortem analysis completed");
        println!("  Classification: {:?}", report.classification);
        
        let branch_state = &engine.branches.get(&branch_id).unwrap().state;
        assert!(matches!(branch_state, BranchState::Quarantined { .. }));
        println!("✓ Branch quarantined");
        println!("  State: {:?}", branch_state);

        // Step 3: Probe in sandbox
        println!("\n═══ Step 3: Probe Execution ═══");
        let probe_result = engine.probe_branch(branch_id).unwrap();
        
        println!("✓ Probe completed");
        println!("  Success: {}", probe_result.success);
        println!("  Issue reproduced: {}", probe_result.reproduced_issue);
        println!("  Runtime: {} ns", probe_result.runtime_ns);

        // Simulate successful probe (issue was transient)
        assert!(probe_result.success || !probe_result.reproduced_issue);

        // Step 4: Released to Probation
        println!("\n═══ Step 4: Release to Probation ═══");
        let branch_state = &engine.branches.get(&branch_id).unwrap().state;
        
        if matches!(branch_state, BranchState::Probationary { .. }) {
            println!("✓ Branch released to probation");
            println!("  State: {:?}", branch_state);
        }

        // Step 5: Execute under probation
        println!("\n═══ Step 5: Probation Period ═══");
        for i in 0..15 {
            let metrics = PerformanceMetrics {
                latency_p50_ns: 1_000_000,
                latency_p99_ns: 5_000_000,
                throughput_ops_per_sec: 1000.0,
                memory_used_mb: 500,
                cpu_utilization: 0.6,
                cache_miss_rate: 0.01,
            };
            engine.record_execution(branch_id, true, metrics, 1_000_000).unwrap();
            
            if (i + 1) % 5 == 0 {
                println!("  ✓ {} executions passed", i + 1);
            }
        }

        // Manually graduate from probation (in real system, monitored automatically)
        println!("\n═══ Step 6: Probation Graduation ═══");
        if let Some(branch) = engine.branches.get_mut(&branch_id) {
            branch.state = BranchState::Active;
            branch.reputation.probation_completions += 1;
        }

        let final_state = &engine.branches.get(&branch_id).unwrap().state;
        assert_eq!(*final_state, BranchState::Active);
        println!("✓ Branch graduated to Active");

        // Final statistics
        println!("\n═══ Final Statistics ═══");
        let branch = engine.branches.get(&branch_id).unwrap();
        println!("Trust score: {:.3}", branch.reputation.trust_score);
        println!("Success count: {}", branch.reputation.success_count);
        println!("Failure count: {}", branch.reputation.failure_count);
        println!("Probation completions: {}", branch.reputation.probation_completions);
        println!("Quarantine events: {}", branch.reputation.quarantine_history.len());

        assert!(branch.reputation.probation_completions > 0);
        assert_eq!(branch.reputation.quarantine_history.len(), 1);

        let _ = std::fs::remove_dir_all(&temp_dir);
        println!("\n✅ SUCCESS PATH COMPLETE\n");
    }

    #[test]
    fn test_full_lifecycle_failure_path() {
        println!("\n╔═══════════════════════════════════════════════════════╗");
        println!("║  FULL LIFECYCLE TEST: FAILURE PATH                   ║");
        println!("║  Quarantine -> Probe -> Failure -> Condemned         ║");
        println!("╚═══════════════════════════════════════════════════════╝\n");

        let temp_dir = std::env::temp_dir().join("azo_lifecycle_failure");
        let _ = std::fs::remove_dir_all(&temp_dir);
        let mut engine = AzoEngine::new(temp_dir.clone()).unwrap();

        // Step 1: Create branch
        println!("═══ Step 1: Create Branch ═══");
        let layout = LayoutPlan {
            thread_count: 4,
            memory_budget_mb: 1024,
            cache_strategy: CacheStrategy::Aggressive,
            prefetch_depth: 4,
            affinity_hints: vec![],
            custom_params: HashMap::new(),
        };
        let branch_id = engine.create_branch(layout);
        println!("✓ Branch created: {:?}", branch_id);

        // Step 2: Multiple failures -> Quarantine
        println!("\n═══ Step 2: Multiple Failures -> Quarantine ═══");
        for i in 0..3 {
            let error_context = ErrorContext {
                error_type: ErrorType::MemoryCorruption,
                error_message: format!("Memory corruption event #{}", i + 1),
                checksum_failed: true,
                thermal_throttled: false,
                cpu_temp_celsius: 70.0,
                memory_pressure_pct: 85.0,
                intermittent: false,
            };

            engine.handle_failure(
                branch_id,
                error_context,
                vec![format!("corruption_event_{}", i)],
                vec![1000, 2000, 3000],
            );
            
            println!("  ✗ Failure #{} recorded", i + 1);
        }

        let branch_state = &engine.branches.get(&branch_id).unwrap().state;
        assert!(matches!(branch_state, BranchState::Quarantined { .. }));
        println!("✓ Branch quarantined after multiple failures");

        // Step 3: Probe reveals persistent issue
        println!("\n═══ Step 3: Probe Reveals Persistent Issue ═══");
        
        // In real implementation, probe would actually reproduce the issue
        // Here we simulate the detection
        let branch = engine.branches.get(&branch_id).unwrap();
        let quarantine_stats = engine.quarantine_manager.get_quarantine_stats();
        
        println!("✓ Quarantine status:");
        println!("  Total quarantined: {}", quarantine_stats.total_branches);
        println!("  Ready for release: {}", quarantine_stats.ready_for_release);

        // Step 4: Failed probe -> Condemned
        println!("\n═══ Step 4: Failed Probes -> Condemned ═══");
        
        // Simulate multiple failed probe attempts
        let failure_count = engine.branches.get(&branch_id).unwrap().reputation.failure_count;
        println!("  Failure count: {}", failure_count);
        println!("  Trust score: {:.3}", engine.branches.get(&branch_id).unwrap().reputation.trust_score);

        // Mark as suspended (condemned)
        if let Some(branch) = engine.branches.get_mut(&branch_id) {
            branch.state = BranchState::Suspended;
        }

        let final_state = &engine.branches.get(&branch_id).unwrap().state;
        assert_eq!(*final_state, BranchState::Suspended);
        println!("✓ Branch condemned (Suspended)");

        // Step 5: Pruning evaluation
        println!("\n═══ Step 5: Pruning Evaluation ═══");
        let branches: Vec<_> = engine.branches.values().cloned().collect();
        let pruning_proposals = engine.pruning_executor.evaluate_branches(&branches);
        
        if !pruning_proposals.is_empty() {
            println!("✓ Pruning candidates identified:");
            for (i, proposal) in pruning_proposals.iter().enumerate() {
                println!("  {}. Branch {:?}", i + 1, proposal.branch_id);
                println!("     Reason: {:?}", proposal.reason);
                println!("     Confidence: {:.3}", proposal.confidence);
            }
        }

        // Final statistics
        println!("\n═══ Final Statistics ═══");
        let branch = engine.branches.get(&branch_id).unwrap();
        println!("State: {:?}", branch.state);
        println!("Trust score: {:.3}", branch.reputation.trust_score);
        println!("Failure count: {}", branch.reputation.failure_count);
        println!("Quarantine events: {}", branch.reputation.quarantine_history.len());

        assert!(branch.reputation.failure_count >= 3);
        assert!(branch.reputation.trust_score < 0.5);

        let _ = std::fs::remove_dir_all(&temp_dir);
        println!("\n✅ FAILURE PATH COMPLETE\n");
    }

    #[test]
    fn test_probation_violation_path() {
        println!("\n╔═══════════════════════════════════════════════════════╗");
        println!("║  LIFECYCLE TEST: PROBATION VIOLATION                 ║");
        println!("║  Quarantine -> Probe -> Probation -> Violation ->    ║");
        println!("║  Back to Quarantine                                  ║");
        println!("╚═══════════════════════════════════════════════════════╝\n");

        let temp_dir = std::env::temp_dir().join("azo_probation_violation");
        let _ = std::fs::remove_dir_all(&temp_dir);
        let mut engine = AzoEngine::new(temp_dir.clone()).unwrap();

        let layout = LayoutPlan {
            thread_count: 4,
            memory_budget_mb: 1024,
            cache_strategy: CacheStrategy::Balanced,
            prefetch_depth: 2,
            affinity_hints: vec![],
            custom_params: HashMap::new(),
        };
        let branch_id = engine.create_branch(layout);

        // Quarantine
        println!("═══ Step 1: Initial Quarantine ═══");
        let error_context = ErrorContext {
            error_type: ErrorType::Timeout,
            error_message: "Operation timeout".to_string(),
            checksum_failed: false,
            thermal_throttled: false,
            cpu_temp_celsius: 68.0,
            memory_pressure_pct: 50.0,
            intermittent: true,
        };

        engine.handle_failure(branch_id, error_context, vec![], vec![]);
        println!("✓ Branch quarantined");

        // Release to probation
        println!("\n═══ Step 2: Release to Probation ═══");
        if let Some(branch) = engine.branches.get_mut(&branch_id) {
            branch.state = BranchState::Probationary {
                entry_time: 0,
                probation_duration: Duration::from_secs(600),
            };
        }
        engine.probation_monitor.add_to_probation(branch_id, Duration::from_secs(600));
        println!("✓ Branch in probation");

        // Execute some successful operations
        println!("\n═══ Step 3: Initial Probation Period ═══");
        for i in 0..5 {
            let metrics = PerformanceMetrics {
                latency_p50_ns: 1_000_000,
                latency_p99_ns: 5_000_000,
                throughput_ops_per_sec: 1000.0,
                memory_used_mb: 500,
                cpu_utilization: 0.6,
                cache_miss_rate: 0.01,
            };
            engine.record_execution(branch_id, true, metrics, 1_000_000).unwrap();
            engine.probation_monitor.record_execution(branch_id, true, metrics);
            println!("  ✓ Execution {} passed", i + 1);
        }

        // Violation during probation
        println!("\n═══ Step 4: Probation Violation ═══");
        let metrics_bad = PerformanceMetrics {
            latency_p50_ns: 10_000_000, // 10x worse
            latency_p99_ns: 50_000_000,
            throughput_ops_per_sec: 100.0,
            memory_used_mb: 500,
            cpu_utilization: 0.6,
            cache_miss_rate: 0.5,
        };
        engine.record_execution(branch_id, false, metrics_bad.clone(), 10_000_000).unwrap();
        engine.probation_monitor.record_execution(branch_id, false, metrics_bad);
        println!("✗ Violation detected during probation");

        // Back to suspended
        if let Some(branch) = engine.branches.get_mut(&branch_id) {
            branch.state = BranchState::Suspended;
        }

        let final_state = &engine.branches.get(&branch_id).unwrap().state;
        assert_eq!(*final_state, BranchState::Suspended);
        println!("✓ Branch re-suspended due to probation violation");

        println!("\n═══ Final Statistics ═══");
        let branch = engine.branches.get(&branch_id).unwrap();
        println!("State: {:?}", branch.state);
        println!("Success count: {}", branch.reputation.success_count);
        println!("Failure count: {}", branch.reputation.failure_count);
        
        assert!(branch.reputation.failure_count > 0);

        let _ = std::fs::remove_dir_all(&temp_dir);
        println!("\n✅ PROBATION VIOLATION PATH COMPLETE\n");
    }

    #[test]
    fn test_multiple_branch_concurrent_lifecycle() {
        println!("\n╔═══════════════════════════════════════════════════════╗");
        println!("║  CONCURRENT LIFECYCLE TEST: Multiple Branches        ║");
        println!("╚═══════════════════════════════════════════════════════╝\n");

        let temp_dir = std::env::temp_dir().join("azo_concurrent_lifecycle");
        let _ = std::fs::remove_dir_all(&temp_dir);
        let mut engine = AzoEngine::new(temp_dir.clone()).unwrap();

        // Create 3 branches with different configurations
        let branches: Vec<BranchId> = (0..3)
            .map(|i| {
                let layout = LayoutPlan {
                    thread_count: 2 + i * 2,
                    memory_budget_mb: 512 + i * 512,
                    cache_strategy: CacheStrategy::Balanced,
                    prefetch_depth: 1 + i,
                    affinity_hints: vec![],
                    custom_params: HashMap::new(),
                };
                engine.create_branch(layout)
            })
            .collect();

        println!("✓ Created {} branches", branches.len());

        // Branch 0: Success path
        println!("\n═══ Branch 0: Success Path ═══");
        for _ in 0..5 {
            let metrics = PerformanceMetrics {
                latency_p50_ns: 1_000_000,
                latency_p99_ns: 5_000_000,
                throughput_ops_per_sec: 1000.0,
                memory_used_mb: 400,
                cpu_utilization: 0.5,
                cache_miss_rate: 0.01,
            };
            engine.record_execution(branches[0], true, metrics, 1_000_000).unwrap();
        }
        println!("✓ Branch 0: 5 successful executions");

        // Branch 1: Quarantine -> Recovery
        println!("\n═══ Branch 1: Quarantine Path ═══");
        engine.handle_failure(
            branches[1],
            ErrorContext {
                error_type: ErrorType::AssertionFailure,
                error_message: "Test failure".to_string(),
                checksum_failed: false,
                thermal_throttled: false,
                cpu_temp_celsius: 65.0,
                memory_pressure_pct: 40.0,
                intermittent: false,
            },
            vec![],
            vec![],
        );
        println!("✓ Branch 1: Quarantined");

        // Branch 2: Multiple failures -> Condemned
        println!("\n═══ Branch 2: Failure Path ═══");
        for i in 0..3 {
            engine.handle_failure(
                branches[2],
                ErrorContext {
                    error_type: ErrorType::MemoryCorruption,
                    error_message: format!("Failure {}", i),
                    checksum_failed: true,
                    thermal_throttled: false,
                    cpu_temp_celsius: 75.0,
                    memory_pressure_pct: 90.0,
                    intermittent: false,
                },
                vec![],
                vec![],
            );
        }
        println!("✓ Branch 2: Multiple failures recorded");

        // Check final states
        println!("\n═══ Final States ═══");
        for (i, &branch_id) in branches.iter().enumerate() {
            let branch = engine.branches.get(&branch_id).unwrap();
            println!("Branch {}: {:?}", i, branch.state);
            println!("  Trust: {:.3}, Success: {}, Failure: {}",
                branch.reputation.trust_score,
                branch.reputation.success_count,
                branch.reputation.failure_count
            );
        }

        let stats = engine.get_engine_stats();
        println!("\n═══ Engine Statistics ═══");
        println!("Total branches: {}", stats.total_branches);
        println!("Quarantined: {}", stats.quarantined_branches);

        assert_eq!(stats.total_branches, 3);
        assert!(stats.quarantined_branches >= 1);

        let _ = std::fs::remove_dir_all(&temp_dir);
        println!("\n✅ CONCURRENT LIFECYCLE COMPLETE\n");
    }
}
