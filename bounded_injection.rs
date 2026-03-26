use crate::types::*;
use crate::pruning::BucketTransferPolicy;
use std::collections::BTreeMap;

/// Cross-Bucket Transfer의 Bounded Prior Injection 증명
/// 
/// 핵심 개념:
/// - Transfer는 target의 prior를 OVERWRITE하지 않음
/// - 대신, source prior를 weight로 조정하여 target prior와 BLEND
/// - Blending은 Bayesian update 형태로 수행
/// - Transfer weight는 hardware similarity와 confidence에 의해 bounded
/// 
/// Formula:
/// ────────────────────────────────────────────────────────────
/// target_prior_new = (1 - w) * target_prior_old + w * source_prior
/// 
/// where:
///   w = transfer_weight * confidence_factor * similarity_factor
///   w ∈ [0, max_injection_weight]
///   max_injection_weight = 0.3 (never more than 30% influence)
/// ────────────────────────────────────────────────────────────

pub struct BoundedPriorInjector {
    policy: BucketTransferPolicy,
    max_injection_weight: f64,
    blend_history: Vec<BlendRecord>,
}

#[derive(Debug, Clone)]
pub struct BlendRecord {
    pub timestamp: u64,
    pub source_bucket: BucketId,
    pub target_bucket: BucketId,
    pub prior_key: String,
    pub source_value: PriorDistribution,
    pub target_value_before: PriorDistribution,
    pub target_value_after: PriorDistribution,
    pub blend_weight: f64,
    pub formula: String,
}

impl BoundedPriorInjector {
    pub fn new() -> Self {
        Self {
            policy: BucketTransferPolicy::new(),
            max_injection_weight: 0.3, // Hard upper bound: 30%
            blend_history: Vec::new(),
        }
    }

    /// Execute bounded prior injection (NOT overwrite)
    /// 
    /// Returns: Updated target bucket with blended priors
    pub fn inject_priors(
        &mut self,
        source: &HardwareBucket,
        target: &mut HardwareBucket,
    ) -> Result<InjectionReport, InjectionError> {
        use crate::pruning::TransferEligibility;

        // Step 1: Check eligibility
        let eligibility = self.policy.can_transfer(source, target);
        let (similarity, base_weight) = match eligibility {
            TransferEligibility::Eligible { similarity, weight } => (similarity, weight),
            TransferEligibility::SourceNotClean { cleanliness } => {
                return Err(InjectionError::SourceNotClean { cleanliness });
            }
            TransferEligibility::TargetNotClean { cleanliness } => {
                return Err(InjectionError::TargetNotClean { cleanliness });
            }
            TransferEligibility::InsufficientSimilarity { similarity } => {
                return Err(InjectionError::InsufficientSimilarity { similarity });
            }
        };

        let mut injections = Vec::new();

        // Step 2: For each prior in source, blend with target
        for (key, source_prior) in &source.priors {
            // Compute bounded blend weight
            let blend_weight = self.compute_blend_weight(
                source_prior,
                base_weight,
                similarity,
            );

            // Get or create target prior
            let target_prior_before = target.priors.get(key).cloned().unwrap_or_else(|| {
                // If target doesn't have this prior, use neutral prior
                PriorDistribution {
                    mean: 0.0,
                    variance: 1.0,
                    sample_count: 0,
                    confidence: 0.0,
                    last_updated: 0,
                }
            });

            // Execute blending (NOT overwrite)
            let target_prior_after = self.blend_priors(
                &target_prior_before,
                source_prior,
                blend_weight,
            );

            // Record blend operation
            let blend_record = BlendRecord {
                timestamp: Self::current_timestamp(),
                source_bucket: source.id,
                target_bucket: target.id,
                prior_key: key.clone(),
                source_value: source_prior.clone(),
                target_value_before: target_prior_before.clone(),
                target_value_after: target_prior_after.clone(),
                blend_weight,
                formula: format!(
                    "target_new = (1 - {:.3}) * target_old + {:.3} * source\n\
                     mean: {:.3} = (1 - {:.3}) * {:.3} + {:.3} * {:.3}\n\
                     variance: {:.3} = (1 - {:.3}) * {:.3} + {:.3} * {:.3}",
                    blend_weight,
                    blend_weight,
                    target_prior_after.mean,
                    blend_weight,
                    target_prior_before.mean,
                    blend_weight,
                    source_prior.mean,
                    target_prior_after.variance,
                    blend_weight,
                    target_prior_before.variance,
                    blend_weight,
                    source_prior.variance
                ),
            };

            self.blend_history.push(blend_record.clone());
            injections.push(blend_record);

            // Update target (blended, not overwritten)
            target.priors.insert(key.clone(), target_prior_after);
        }

        Ok(InjectionReport {
            source_bucket: source.id,
            target_bucket: target.id,
            total_priors_injected: injections.len(),
            average_blend_weight: injections.iter().map(|i| i.blend_weight).sum::<f64>()
                / injections.len() as f64,
            injections,
        })
    }

    /// Compute bounded blend weight
    /// 
    /// Weight is product of:
    /// - Base transfer weight (from policy)
    /// - Source confidence (high confidence = more influence)
    /// - Hardware similarity (high similarity = more influence)
    /// 
    /// Bounded by max_injection_weight (30%)
    fn compute_blend_weight(
        &self,
        source_prior: &PriorDistribution,
        base_weight: f64,
        similarity: f64,
    ) -> f64 {
        let weight = base_weight * source_prior.confidence * similarity;
        weight.min(self.max_injection_weight) // Hard cap at 30%
    }

    /// Blend two priors (Bayesian-style update)
    /// 
    /// Formula:
    ///   mean_new = (1 - w) * mean_target + w * mean_source
    ///   var_new = (1 - w) * var_target + w * var_source
    ///   confidence_new = (1 - w) * conf_target + w * conf_source
    ///   sample_count_new = sample_count_target + weighted_sample_count_source
    fn blend_priors(
        &self,
        target: &PriorDistribution,
        source: &PriorDistribution,
        weight: f64,
    ) -> PriorDistribution {
        PriorDistribution {
            mean: (1.0 - weight) * target.mean + weight * source.mean,
            variance: (1.0 - weight) * target.variance + weight * source.variance,
            sample_count: target.sample_count + (source.sample_count as f64 * weight) as usize,
            confidence: ((1.0 - weight) * target.confidence + weight * source.confidence).min(1.0),
            last_updated: Self::current_timestamp(),
        }
    }

    fn current_timestamp() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    pub fn print_injection_report(&self, report: &InjectionReport) {
        println!("\n╔═══════════════════════════════════════════╗");
        println!("║   BOUNDED PRIOR INJECTION REPORT          ║");
        println!("╚═══════════════════════════════════════════╝");
        println!("Source bucket:  {:?}", report.source_bucket);
        println!("Target bucket:  {:?}", report.target_bucket);
        println!("Priors injected: {}", report.total_priors_injected);
        println!("Average blend weight: {:.3} (max: 0.300)", report.average_blend_weight);
        println!("\nDetailed Injections:");

        for (i, injection) in report.injections.iter().enumerate() {
            println!("\n─────────────────────────────────────────");
            println!("Injection #{}: {}", i + 1, injection.prior_key);
            println!("─────────────────────────────────────────");
            println!("Blend weight: {:.3}", injection.blend_weight);
            println!("\nBefore:");
            println!("  Target mean: {:.3}, variance: {:.3}, samples: {}",
                injection.target_value_before.mean,
                injection.target_value_before.variance,
                injection.target_value_before.sample_count
            );
            println!("  Source mean: {:.3}, variance: {:.3}, samples: {}",
                injection.source_value.mean,
                injection.source_value.variance,
                injection.source_value.sample_count
            );
            println!("\nAfter (BLENDED, not overwritten):");
            println!("  Target mean: {:.3}, variance: {:.3}, samples: {}",
                injection.target_value_after.mean,
                injection.target_value_after.variance,
                injection.target_value_after.sample_count
            );
            println!("\nFormula:");
            println!("{}", injection.formula);
        }
    }
}

#[derive(Debug)]
pub struct InjectionReport {
    pub source_bucket: BucketId,
    pub target_bucket: BucketId,
    pub total_priors_injected: usize,
    pub average_blend_weight: f64,
    pub injections: Vec<BlendRecord>,
}

#[derive(Debug)]
pub enum InjectionError {
    SourceNotClean { cleanliness: f64 },
    TargetNotClean { cleanliness: f64 },
    InsufficientSimilarity { similarity: f64 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_injection_not_overwrite() {
        let mut injector = BoundedPriorInjector::new();

        // Create source bucket with priors
        let mut source = HardwareBucket::new(
            BucketId(1),
            HardwareFingerprint {
                cpu_model: "Intel Xeon".to_string(),
                core_count: 8,
                l1_cache_kb: 32,
                l2_cache_kb: 256,
                l3_cache_kb: 8192,
                memory_gb: 32,
                numa_topology: vec![0],
                feature_flags: vec![],
            },
        );
        source.integrity_hash = "valid".to_string();

        source.priors.insert(
            "latency_mean".to_string(),
            PriorDistribution {
                mean: 100.0, // Source: 100ms
                variance: 10.0,
                sample_count: 1000,
                confidence: 0.9,
                last_updated: 0,
            },
        );

        // Create target bucket with different priors
        let mut target = HardwareBucket::new(
            BucketId(2),
            source.fingerprint.clone(),
        );
        target.integrity_hash = "valid".to_string();

        target.priors.insert(
            "latency_mean".to_string(),
            PriorDistribution {
                mean: 50.0, // Target: 50ms (different!)
                variance: 5.0,
                sample_count: 500,
                confidence: 0.8,
                last_updated: 0,
            },
        );

        let report = injector.inject_priors(&source, &mut target).unwrap();

        // Verify NOT overwrite
        let target_after = target.priors.get("latency_mean").unwrap();
        
        // Target should be BETWEEN original target and source (blended)
        assert!(target_after.mean > 50.0); // Moved towards source
        assert!(target_after.mean < 100.0); // But not equal to source
        
        // Verify blend weight is bounded
        assert!(report.average_blend_weight <= 0.3); // Max 30%

        println!("\nProof of Bounded Injection (NOT Overwrite):");
        println!("Source mean:        100.0");
        println!("Target mean before:  50.0");
        println!("Target mean after:   {:.3}", target_after.mean);
        println!("Blend weight:        {:.3}", report.average_blend_weight);
        println!("\nTarget is BETWEEN original and source (blended, not overwritten)");

        injector.print_injection_report(&report);
    }

    #[test]
    fn test_blend_weight_bounded_by_max() {
        let injector = BoundedPriorInjector::new();

        // Even with high confidence and similarity, weight is capped
        let source_prior = PriorDistribution {
            mean: 100.0,
            variance: 10.0,
            sample_count: 10000,
            confidence: 1.0, // Perfect confidence
            last_updated: 0,
        };

        let weight = injector.compute_blend_weight(
            &source_prior,
            1.0, // Max base weight
            1.0, // Perfect similarity
        );

        // Should be capped at max_injection_weight
        assert_eq!(weight, 0.3);
        println!("Blend weight with perfect conditions: {:.3} (capped at 0.3)", weight);
    }

    #[test]
    fn test_multiple_injections_preserve_independence() {
        let mut injector = BoundedPriorInjector::new();

        let mut source = HardwareBucket::new(
            BucketId(1),
            HardwareFingerprint {
                cpu_model: "Intel Xeon".to_string(),
                core_count: 8,
                l1_cache_kb: 32,
                l2_cache_kb: 256,
                l3_cache_kb: 8192,
                memory_gb: 32,
                numa_topology: vec![0],
                feature_flags: vec![],
            },
        );
        source.integrity_hash = "valid".to_string();

        // Add multiple priors
        source.priors.insert("latency".to_string(), PriorDistribution {
            mean: 100.0,
            variance: 10.0,
            sample_count: 1000,
            confidence: 0.9,
            last_updated: 0,
        });
        source.priors.insert("throughput".to_string(), PriorDistribution {
            mean: 5000.0,
            variance: 500.0,
            sample_count: 1000,
            confidence: 0.85,
            last_updated: 0,
        });
        source.priors.insert("memory".to_string(), PriorDistribution {
            mean: 2048.0,
            variance: 200.0,
            sample_count: 1000,
            confidence: 0.8,
            last_updated: 0,
        });

        let mut target = HardwareBucket::new(
            BucketId(2),
            source.fingerprint.clone(),
        );
        target.integrity_hash = "valid".to_string();

        target.priors.insert("latency".to_string(), PriorDistribution {
            mean: 50.0,
            variance: 5.0,
            sample_count: 500,
            confidence: 0.7,
            last_updated: 0,
        });
        target.priors.insert("throughput".to_string(), PriorDistribution {
            mean: 3000.0,
            variance: 300.0,
            sample_count: 500,
            confidence: 0.6,
            last_updated: 0,
        });

        let report = injector.inject_priors(&source, &mut target).unwrap();

        // Verify each prior was blended independently
        assert_eq!(report.total_priors_injected, 3);

        // Latency should be blended
        let latency_after = target.priors.get("latency").unwrap();
        assert!(latency_after.mean > 50.0 && latency_after.mean < 100.0);

        // Throughput should be blended independently
        let throughput_after = target.priors.get("throughput").unwrap();
        assert!(throughput_after.mean > 3000.0 && throughput_after.mean < 5000.0);

        // Memory should be created (target didn't have it)
        let memory_after = target.priors.get("memory").unwrap();
        assert!(memory_after.mean > 0.0); // Should be blend of 0.0 and 2048.0

        injector.print_injection_report(&report);
    }

    #[test]
    fn test_blend_formula_correctness() {
        let injector = BoundedPriorInjector::new();

        let target = PriorDistribution {
            mean: 50.0,
            variance: 5.0,
            sample_count: 100,
            confidence: 0.8,
            last_updated: 0,
        };

        let source = PriorDistribution {
            mean: 100.0,
            variance: 10.0,
            sample_count: 200,
            confidence: 0.9,
            last_updated: 0,
        };

        let weight = 0.2; // 20% blend

        let blended = injector.blend_priors(&target, &source, weight);

        // Verify formula: mean_new = (1 - w) * target + w * source
        let expected_mean = 0.8 * 50.0 + 0.2 * 100.0; // 40 + 20 = 60
        assert!((blended.mean - expected_mean).abs() < 1e-6);

        let expected_variance = 0.8 * 5.0 + 0.2 * 10.0; // 4 + 2 = 6
        assert!((blended.variance - expected_variance).abs() < 1e-6);

        println!("Blend formula verification:");
        println!("  Target mean: {}, Source mean: {}", target.mean, source.mean);
        println!("  Blend weight: {}", weight);
        println!("  Expected mean: {} = 0.8 * {} + 0.2 * {}", expected_mean, target.mean, source.mean);
        println!("  Actual mean: {}", blended.mean);
        println!("  ✓ Formula correct");
    }

    #[test]
    fn test_injection_with_low_similarity_rejected() {
        let mut injector = BoundedPriorInjector::new();

        let mut source = HardwareBucket::new(
            BucketId(1),
            HardwareFingerprint {
                cpu_model: "Intel Core i5".to_string(),
                core_count: 4,
                l1_cache_kb: 32,
                l2_cache_kb: 256,
                l3_cache_kb: 6144,
                memory_gb: 16,
                numa_topology: vec![0],
                feature_flags: vec![],
            },
        );
        source.integrity_hash = "valid".to_string();

        let mut target = HardwareBucket::new(
            BucketId(2),
            HardwareFingerprint {
                cpu_model: "AMD EPYC".to_string(), // Very different
                core_count: 64,
                l1_cache_kb: 32,
                l2_cache_kb: 512,
                l3_cache_kb: 262144,
                memory_gb: 256,
                numa_topology: vec![0, 1, 2, 3],
                feature_flags: vec![],
            },
        );
        target.integrity_hash = "valid".to_string();

        let result = injector.inject_priors(&source, &mut target);

        // Should fail due to low similarity
        assert!(result.is_err());
        match result.unwrap_err() {
            InjectionError::InsufficientSimilarity { similarity } => {
                println!("Injection rejected: similarity too low ({:.3})", similarity);
                assert!(similarity < 0.7);
            }
            _ => panic!("Expected InsufficientSimilarity error"),
        }
    }

    #[test]
    fn test_blend_history_tracking() {
        let mut injector = BoundedPriorInjector::new();

        let mut source = HardwareBucket::new(BucketId(1), HardwareFingerprint {
            cpu_model: "Test".to_string(),
            core_count: 4,
            l1_cache_kb: 32,
            l2_cache_kb: 256,
            l3_cache_kb: 8192,
            memory_gb: 16,
            numa_topology: vec![0],
            feature_flags: vec![],
        });
        source.integrity_hash = "valid".to_string();
        source.priors.insert("test".to_string(), PriorDistribution {
            mean: 100.0,
            variance: 10.0,
            sample_count: 100,
            confidence: 0.9,
            last_updated: 0,
        });

        let mut target = source.clone();
        target.id = BucketId(2);

        injector.inject_priors(&source, &mut target).unwrap();

        // Verify history was recorded
        assert_eq!(injector.blend_history.len(), 1);
        let record = &injector.blend_history[0];
        assert_eq!(record.source_bucket, BucketId(1));
        assert_eq!(record.target_bucket, BucketId(2));
        assert!(!record.formula.is_empty());
    }
}
