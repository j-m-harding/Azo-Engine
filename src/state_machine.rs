use crate::types::*;
use std::time::Duration;

/// BranchState 상태 전이 관리자
/// 
/// 상태 전이 다이어그램:
/// ```
///     ┌─────────────┐
///     │   Active    │◄──────────────────────────┐
///     └──────┬──────┘                            │
///            │                                   │
///            │ failure detected                  │ graduation
///            ▼                                   │
///     ┌─────────────┐                     ┌──────┴──────┐
///     │ Quarantined │────probe success───►│ Probationary│
///     └──────┬──────┘                     └──────┬──────┘
///            │                                   │
///            │ probe failed                      │ violation
///            │ or timeout                        │
///            ▼                                   ▼
///     ┌─────────────┐                     ┌─────────────┐
///     │  Suspended  │                     │  Suspended  │
///     └──────┬──────┘                     └──────┬──────┘
///            │                                   │
///            │ manual review                     │
///            ▼                                   │
///     ┌─────────────┐                            │
///     │  Archived   │◄───────────────────────────┘
///     └─────────────┘
/// ```
///
/// 불가능한 전이:
/// - Active → Probationary (반드시 Quarantine을 거쳐야 함)
/// - Quarantined → Active (반드시 Probation을 거쳐야 함)
/// - Archived → * (최종 상태, 탈출 불가)
/// - Suspended → Active (반드시 Probation을 거쳐야 함)
pub struct BranchStateMachine;

#[derive(Debug, PartialEq)]
pub enum StateTransition {
    // Valid transitions
    FailureDetected { reason: QuarantineReason },
    ProbeSuccess { probation_duration: Duration },
    ProbeFailed,
    ProbationViolation,
    ProbationGraduated,
    ManualArchive,
    
    // Invalid transitions (compile-time prevented)
    // DirectToActive,  // Not allowed from Quarantine
    // SkipQuarantine,  // Not allowed to Probation
}

#[derive(Debug, PartialEq)]
pub enum TransitionError {
    InvalidTransition { from: String, to: String, reason: String },
    MissingPrecondition { condition: String },
    StateImmutable { state: String },
}

impl BranchStateMachine {
    /// Apply state transition with validation
    pub fn transition(
        current: &BranchState,
        transition: StateTransition,
    ) -> Result<BranchState, TransitionError> {
        match (current, transition) {
            // Active → Quarantined (only valid exit from Active)
            (BranchState::Active, StateTransition::FailureDetected { reason }) => {
                Ok(BranchState::Quarantined {
                    reason,
                    since: Self::current_timestamp(),
                })
            }

            // Quarantined → Probationary (only after successful probe)
            (
                BranchState::Quarantined { .. },
                StateTransition::ProbeSuccess { probation_duration },
            ) => Ok(BranchState::Probationary {
                entry_time: Self::current_timestamp(),
                probation_duration,
            }),

            // Quarantined → Suspended (probe failed or timeout)
            (BranchState::Quarantined { .. }, StateTransition::ProbeFailed) => {
                Ok(BranchState::Suspended)
            }

            // Probationary → Active (graduation after successful period)
            (BranchState::Probationary { .. }, StateTransition::ProbationGraduated) => {
                Ok(BranchState::Active)
            }

            // Probationary → Suspended (violation during probation)
            (BranchState::Probationary { .. }, StateTransition::ProbationViolation) => {
                Ok(BranchState::Suspended)
            }

            // Suspended → Archived (final state)
            (BranchState::Suspended, StateTransition::ManualArchive) => {
                Ok(BranchState::Archived)
            }

            // Any state → Archived (manual override)
            (_, StateTransition::ManualArchive) => Ok(BranchState::Archived),

            // INVALID: Archived is immutable
            (BranchState::Archived, _) => Err(TransitionError::StateImmutable {
                state: "Archived".to_string(),
            }),

            // INVALID: Active cannot directly go to Probationary
            (BranchState::Active, StateTransition::ProbeSuccess { .. }) => {
                Err(TransitionError::InvalidTransition {
                    from: "Active".to_string(),
                    to: "Probationary".to_string(),
                    reason: "Must pass through Quarantine first".to_string(),
                })
            }

            // INVALID: Quarantined cannot directly go to Active
            (BranchState::Quarantined { .. }, StateTransition::ProbationGraduated) => {
                Err(TransitionError::InvalidTransition {
                    from: "Quarantined".to_string(),
                    to: "Active".to_string(),
                    reason: "Must pass through Probation first".to_string(),
                })
            }

            // INVALID: Suspended cannot go directly to Active
            (BranchState::Suspended, StateTransition::ProbationGraduated) => {
                Err(TransitionError::InvalidTransition {
                    from: "Suspended".to_string(),
                    to: "Active".to_string(),
                    reason: "Cannot restore suspended branch to active".to_string(),
                })
            }

            // Catch-all for other invalid transitions
            _ => Err(TransitionError::InvalidTransition {
                from: format!("{:?}", current),
                to: format!("{:?}", transition),
                reason: "Invalid state transition".to_string(),
            }),
        }
    }

    /// Validate transition without applying it
    pub fn can_transition(current: &BranchState, transition: &StateTransition) -> bool {
        Self::transition(current, transition.clone()).is_ok()
    }

    /// Get all valid transitions from current state
    pub fn valid_transitions(current: &BranchState) -> Vec<String> {
        match current {
            BranchState::Active => vec!["FailureDetected".to_string(), "ManualArchive".to_string()],
            BranchState::Quarantined { .. } => {
                vec![
                    "ProbeSuccess".to_string(),
                    "ProbeFailed".to_string(),
                    "ManualArchive".to_string(),
                ]
            }
            BranchState::Probationary { .. } => {
                vec![
                    "ProbationGraduated".to_string(),
                    "ProbationViolation".to_string(),
                    "ManualArchive".to_string(),
                ]
            }
            BranchState::Suspended => vec!["ManualArchive".to_string()],
            BranchState::Archived => vec![],
        }
    }

    fn current_timestamp() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }
}

// Implement Clone for StateTransition to enable validation
impl Clone for StateTransition {
    fn clone(&self) -> Self {
        match self {
            Self::FailureDetected { reason } => Self::FailureDetected {
                reason: reason.clone(),
            },
            Self::ProbeSuccess { probation_duration } => Self::ProbeSuccess {
                probation_duration: *probation_duration,
            },
            Self::ProbeFailed => Self::ProbeFailed,
            Self::ProbationViolation => Self::ProbationViolation,
            Self::ProbationGraduated => Self::ProbationGraduated,
            Self::ManualArchive => Self::ManualArchive,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_transitions() {
        // Active → Quarantined
        let active = BranchState::Active;
        let result = BranchStateMachine::transition(
            &active,
            StateTransition::FailureDetected {
                reason: QuarantineReason::PerformanceAnomaly { spike_magnitude: 5.0 },
            },
        );
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), BranchState::Quarantined { .. }));

        // Quarantined → Probationary
        let quarantined = BranchState::Quarantined {
            reason: QuarantineReason::PerformanceAnomaly { spike_magnitude: 5.0 },
            since: 0,
        };
        let result = BranchStateMachine::transition(
            &quarantined,
            StateTransition::ProbeSuccess {
                probation_duration: Duration::from_secs(600),
            },
        );
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), BranchState::Probationary { .. }));

        // Probationary → Active
        let probationary = BranchState::Probationary {
            entry_time: 0,
            probation_duration: Duration::from_secs(600),
        };
        let result = BranchStateMachine::transition(&probationary, StateTransition::ProbationGraduated);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), BranchState::Active);
    }

    #[test]
    fn test_invalid_active_to_probationary() {
        // INVALID: Active cannot skip Quarantine and go directly to Probationary
        let active = BranchState::Active;
        let result = BranchStateMachine::transition(
            &active,
            StateTransition::ProbeSuccess {
                probation_duration: Duration::from_secs(600),
            },
        );
        
        assert!(result.is_err());
        match result.unwrap_err() {
            TransitionError::InvalidTransition { from, to, reason } => {
                assert_eq!(from, "Active");
                assert_eq!(to, "Probationary");
                assert!(reason.contains("Quarantine"));
            }
            _ => panic!("Expected InvalidTransition error"),
        }
    }

    #[test]
    fn test_invalid_quarantine_to_active() {
        // INVALID: Quarantined cannot skip Probation and go directly to Active
        let quarantined = BranchState::Quarantined {
            reason: QuarantineReason::PerformanceAnomaly { spike_magnitude: 5.0 },
            since: 0,
        };
        let result = BranchStateMachine::transition(&quarantined, StateTransition::ProbationGraduated);
        
        assert!(result.is_err());
        match result.unwrap_err() {
            TransitionError::InvalidTransition { from, to, reason } => {
                assert_eq!(from, "Quarantined");
                assert_eq!(to, "Active");
                assert!(reason.contains("Probation"));
            }
            _ => panic!("Expected InvalidTransition error"),
        }
    }

    #[test]
    fn test_archived_is_immutable() {
        // INVALID: Archived state cannot transition anywhere
        let archived = BranchState::Archived;
        
        let result = BranchStateMachine::transition(
            &archived,
            StateTransition::FailureDetected {
                reason: QuarantineReason::PerformanceAnomaly { spike_magnitude: 5.0 },
            },
        );
        
        assert!(result.is_err());
        match result.unwrap_err() {
            TransitionError::StateImmutable { state } => {
                assert_eq!(state, "Archived");
            }
            _ => panic!("Expected StateImmutable error"),
        }
    }

    #[test]
    fn test_full_lifecycle_path() {
        // Valid path: Active → Quarantined → Probationary → Active
        let mut state = BranchState::Active;

        // Step 1: Active → Quarantined
        state = BranchStateMachine::transition(
            &state,
            StateTransition::FailureDetected {
                reason: QuarantineReason::PerformanceAnomaly { spike_magnitude: 5.0 },
            },
        )
        .unwrap();
        assert!(matches!(state, BranchState::Quarantined { .. }));

        // Step 2: Quarantined → Probationary
        state = BranchStateMachine::transition(
            &state,
            StateTransition::ProbeSuccess {
                probation_duration: Duration::from_secs(600),
            },
        )
        .unwrap();
        assert!(matches!(state, BranchState::Probationary { .. }));

        // Step 3: Probationary → Active
        state = BranchStateMachine::transition(&state, StateTransition::ProbationGraduated).unwrap();
        assert_eq!(state, BranchState::Active);
    }

    #[test]
    fn test_failure_path_to_archive() {
        // Failure path: Active → Quarantined → Suspended → Archived
        let mut state = BranchState::Active;

        // Step 1: Active → Quarantined
        state = BranchStateMachine::transition(
            &state,
            StateTransition::FailureDetected {
                reason: QuarantineReason::CorrectnessFailure {
                    test_case: "test_1".to_string(),
                },
            },
        )
        .unwrap();

        // Step 2: Quarantined → Suspended (probe failed)
        state = BranchStateMachine::transition(&state, StateTransition::ProbeFailed).unwrap();
        assert_eq!(state, BranchState::Suspended);

        // Step 3: Suspended → Archived
        state = BranchStateMachine::transition(&state, StateTransition::ManualArchive).unwrap();
        assert_eq!(state, BranchState::Archived);
    }

    #[test]
    fn test_valid_transitions_list() {
        assert_eq!(
            BranchStateMachine::valid_transitions(&BranchState::Active),
            vec!["FailureDetected", "ManualArchive"]
        );

        let quarantined = BranchState::Quarantined {
            reason: QuarantineReason::PerformanceAnomaly { spike_magnitude: 5.0 },
            since: 0,
        };
        assert_eq!(
            BranchStateMachine::valid_transitions(&quarantined),
            vec!["ProbeSuccess", "ProbeFailed", "ManualArchive"]
        );

        assert_eq!(
            BranchStateMachine::valid_transitions(&BranchState::Archived),
            Vec::<String>::new()
        );
    }

    #[test]
    fn test_can_transition_validation() {
        let active = BranchState::Active;
        
        // Valid
        assert!(BranchStateMachine::can_transition(
            &active,
            &StateTransition::FailureDetected {
                reason: QuarantineReason::PerformanceAnomaly { spike_magnitude: 5.0 },
            }
        ));

        // Invalid
        assert!(!BranchStateMachine::can_transition(
            &active,
            &StateTransition::ProbeSuccess {
                probation_duration: Duration::from_secs(600),
            }
        ));
    }
}
