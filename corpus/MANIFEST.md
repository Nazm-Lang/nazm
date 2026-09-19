# Task corpus

Three development tasks. **Development set only** — these are opened freely during
implementation, which is exactly why they can never be used for the gate. Calibration and
holdout corpora are separate bundles, built later and not present in this repository.

Each task directory holds:

```
task.toml          requirements, caps, and the arm-neutral statement of the work
visible/           tests the agent can see and run
acceptance/        independent tests the agent never sees, run outside its writable tree
reference/         a known-good solution, used to prove the acceptance runner accepts
broken/            deliberately wrong solutions, used to prove it rejects
```

`reference/` and `broken/` are what make the corpus a *test of the harness* rather than
only an input to it. A corpus that has never accepted a known-good solution and rejected a
known-bad one has not been shown to discriminate at all.

## The three tasks

| id | family | why this one |
| --- | --- | --- |
| `crud-01` | ValidatedCrudApi | Baseline usability: routing, JSON, validation, error shape |
| `reservation-01` | TransactionalReservation | Concurrency correctness. Ownership prevents data races; it does **not** prevent two valid transactions overselling. The acceptance tests hit it concurrently for exactly that reason |
| `maintenance-01` | ExistingServiceChange | A change to an existing service. The gate-critical family and the one most benchmarks omit |

## Arm neutrality

`task.toml` states requirements and observable behaviour only — never an implementation
shape. The same statement is given to every arm.

The helper surface each arm may use is recorded in `helpers.toml` per arm and published
with results. If one arm ships `reserveInventorySafely()` while another must hand-roll the
transaction, the experiment measures library abstraction rather than language.
