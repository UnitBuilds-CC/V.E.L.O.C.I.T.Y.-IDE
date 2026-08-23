# CAPTCHA Solver Engine

## Classification
- **Category**: Browser Engine / Agent Automation
- **Files**: velocity-browser/src/engine/captcha/ (14 files)
- **Criticality**: High — enables agent browser automation

## Summary

Modular CAPTCHA solving engine with fingerprint-first cost escalation: visual fingerprinting → template replay (zero tokens) → DOM observation → rule engine → LLM fallback. Supports hCaptcha, reCAPTCHA, Cloudflare Turnstile.

## Solve Loop

1. Rasterize → PixelBuffer
2. Visual fingerprint (~microseconds, free)
3. Template cache lookup (zero tokens on hit)
4. DOM observation → ChallengeSnapshot
5. Provider fingerprinting
6. State machine execution
7. On success: store template for future replay

## Key Components

- `CaptchaOrchestrator` — 9-step solve coordinator
- `VisualFingerprinter` — Pixel → perceptual hash + archetype
- `TemplateStore` — Learned solution cache
- `RuleEngine` — Deterministic solver before LLM fallback
- `SplineExtractor` + `SplineLibrary` — Shape recognition
- `ShadowMatcher` — Azure-style silhouette matching
- `TemporalMonitor` — Animated challenge detection
