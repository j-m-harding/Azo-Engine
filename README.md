![Tests](https://img.shields.io/badge/tests-46%20passing-brightgreen)
![Language](https://img.shields.io/badge/language-Rust-orange)
![License](https://img.shields.io/badge/license-MIT-blue)

# Azo-Engine
A Rust-based execution engine implementing judicial fault isolation and Bayesian-driven state inheritance.

**An evolutionary execution runtime featuring judicial fault isolation and Bayesian-driven state inheritance.**

AZO Engine is an experimental execution layer that treats system failures as high-entropy state data rather than binary errors. By implementing a formal **Judicial Process** for anomaly resolution and a **Bayesian Inheritance** mechanism for cross-node knowledge transfer, it builds a resilient runtime that evolves alongside its physical hardware environment.

---

## Core Architecture

```mermaid
graph TD
    A[Execution Start] --> B{Anomaly Detected?}
    B -- No --> C[Update Reputation & Continue]
    B -- Yes --> D[Quarantine Branch]
    D --> E[Judicial Probing.rs]
    E --> F{Deterministic?}
    F -- Hardware Noise --> G[Bayesian State Recovery]
    F -- Software Bug --> H[Raise Formal Exception]
    G --> I[Heritage Transfer to New Node]
    H --> J[System Rollback] ```


### 1. Judicial Fault Isolation
Moving beyond traditional exception handling, AZO Engine implements a structured **Quarantine** workflow. When an execution branch exhibits non-deterministic behavior, it is isolated for active probing (`probing.rs`) to determine whether the root cause is a software regression or transient hardware-induced noise.

### 2. Bayesian Heritage Transfer
Utilizing the logic in `bounded_injection.rs`, the engine distills optimized execution priors from decommissioned or failing nodes. This **Heritage** is then injected into new instances, drastically reducing **Warm-up Time** and allowing the system to maintain "ancestral" performance optimizations across generations.

### 3. Geodesic Reputation Scoring
The system employs a dynamic decay formula (`reputation.rs`) that accounts for environmental drift and hardware degradation. Reputation scores are not static; they are continuously re-evaluated based on workload transitions and temporal decay constants to mitigate execution risks in aging infrastructure.

---

## Project Structure

* **`types.rs`**: Core domain models, state definitions, and hardware fingerprints.
* **`state_machine.rs`**: Formal state transition logic validated by 46+ deterministic test cases.
* **`reputation.rs`**: Dynamic trust engine featuring environmental and temporal decay multipliers.
* **`bounded_injection.rs`**: Bayesian update logic for prior blending and genetic state inheritance.
* **`lifecycle_integration.rs`**: End-to-end integration of the execution lifecycle and node migration.
* **`minimal_reproduction.rs`**: Utilities for failure re-enactment and counter-example generation.

---

## Getting Started

### Prerequisites
* Rust 1.75.0 or higher
* Cargo (Rust package manager)

### Installation & Test
```bash
# Clone the repository
git clone [https://github.com/your-username/azo-engine.git](https://github.com/your-username/azo-engine.git)

# Build for release
cargo build --release

# Execute the judicial test suite (46 cases)
cargo test
