# Agent Test Writer

## Description
Write meaningful behavior tests for Velocity workspace modules that verify domain properties, invariants, and relationships — not just that values stay in bounds. Use when writing tests for compiler modules, browser engine, agent loop, editor features, or any domain logic.

## When to Use
- Writing tests for a new or existing module
- Replacing vanity tests (smoke tests, trivial assertions) with meaningful tests
- Reviewing test quality in a module
- Adding tests before committing changes

## The Three Patterns

### Pattern A: Behavior Tests (Gold Standard)
Test **why** the system behaves the way it should. Verify domain properties and invariants:
```rust
// "does the NDA lexer produce more tokens for more complex input?"
#[test] fn test_lexer_token_count_scales_with_input() {
    let small = lex("fn main() {}");
    let large = lex("fn main() { let x = 1; let y = 2; x + y }");
    assert!(large.tokens.len() > small.tokens.len());
}
```

### Pattern B: Bounds Tests (Low Value)
Only verify the value didn't overflow. Rust's type system already guarantees this.
```rust
// LOW VALUE: just checking it didn't panic
#[test] fn test_parse_doesnt_crash() { parse(input); }
```

### Pattern C: Smoke Tests (Baseline Only)
Verify construction doesn't panic. Fine as baseline, insufficient alone.
```rust
#[test] fn test_new() { let x = Module::new(); assert!(x.is_active()); }
```

## Behavior Test Templates

### 1. Monotonicity: "Does X increase with Y?"
```rust
#[test] fn test_more_complex_input_produces_more_output() {
    let simple = process("fn a() {}");
    let complex = process("fn a() { let x = 1; let y = 2; }");
    assert!(complex.output.len() > simple.output.len());
}
```

### 2. Determinism: "Same inputs produce same outputs?"
```rust
#[test] fn test_deterministic() {
    let r1 = process(INPUT);
    let r2 = process(INPUT);
    assert_eq!(r1, r2);
}
```

### 3. Round-Trip: "Serialize then deserialize gives original?"
```rust
#[test] fn test_nda_round_trip() {
    let original = NdaTriple { subject: 42, predicate: 7, object_kind: 1, object_value: [0; 11] };
    let bytes = original.serialize();
    let decoded = NdaTriple::deserialize(&bytes).unwrap();
    assert_eq!(original.subject, decoded.subject);
    assert_eq!(original.predicate, decoded.predicate);
}
```

### 4. Error Handling: "Invalid input produces error, not panic?"
```rust
#[test] fn test_invalid_input_returns_error() {
    let result = parse("invalid {{{ syntax");
    assert!(result.is_err());
}
```

### 5. Ordering: "Are variants correctly ranked?"
```rust
#[test] fn test_provider_priority_ordering() {
    assert!(Provider::Cloudflare.priority() < Provider::OpenRouter.priority());
    assert!(Provider::OpenRouter.priority() < Provider::Azure.priority());
    assert!(Provider::Azure.priority() < Provider::Ollama.priority());
}
```

### 6. Isolation: "Does mutating clone leave original intact?"
```rust
#[test] fn test_clone_independent() {
    let mut a = State::new();
    a.push(42);
    let mut b = a.clone();
    b.push(99);
    assert_eq!(a.len(), 1);
    assert_eq!(b.len(), 2);
}
```

### 7. Edge Case: "Does empty/zero produce expected output?"
```rust
#[test] fn test_empty_input_produces_empty_output() {
    let result = process("");
    assert!(result.is_empty());
}
```

### 8. Failover: "Does fallback activate on primary failure?"
```rust
#[test] fn test_failover_on_primary_error() {
    let mut chain = ProviderChain::new();
    chain.mark_failed(Provider::Cloudflare);
    let selected = chain.select_provider();
    assert_eq!(selected, Provider::OpenRouter);
}
```

## Anti-Patterns to Avoid

| Anti-Pattern | Why It's Bad | Fix |
|---|---|---|
| `test_new()` only | Doesn't test any behavior | Add tests for each public method |
| `assert!(x.is_ok())` without checking value | Doesn't verify correctness | Assert on specific expected values |
| 50 tests that all call `process(seed_N)` | One test with a loop suffices | Replace with property-based tests |
| Only testing the happy path | Misses error handling bugs | Add error case tests |

## Target: 15-25 Behavior Tests Per Module

A well-tested module should have:
- 3-5 monotonicity tests (more X → more Y)
- 2-3 determinism tests (same input → same output)
- 2-3 round-trip tests (serialize/deserialize)
- 2-3 error handling tests (invalid input → error)
- 2-3 edge case tests (empty, zero, max)
- 1-2 failover tests (for agent modules)
- 1-2 clone independence tests

## Quality Checklist

Before committing tests, verify each test:
- [ ] Tests a domain-specific behavior, not just bounds
- [ ] Would fail if the domain logic were broken
- [ ] Has a descriptive name explaining what it tests
- [ ] Uses two contrasting configurations (not just one)
- [ ] Is not a copy-paste of another test with a different number
