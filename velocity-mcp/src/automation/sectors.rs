//! Deterministic sector decomposition for repo-scale rewrites.
//!
//! [`crate::registry::team_tools::repo_rewrite`] used to emit a plan whose
//! "port modules by domain" phase was a single line covering the whole
//! repository. On a codebase the size of LiteLLM (38 MB of non-test Python,
//! ~1.1M lines) that phase is not a task — it is the entire job, handed to one
//! agent with one context window, and it cannot be scheduled, parallelised,
//! resumed or gated.
//!
//! This module turns a cloned tree into *sectors*: dependency-ordered groups of
//! source files small enough for one dispatch to own, with oversized files
//! pre-split into line-range chunks. It is pure filesystem work — no model, no
//! network — so it is reproducible, which is what makes it unit-testable and
//! safe to re-run on resume.
//!
//! Measured against a real LiteLLM clone (see the `#[ignore]`d scale check in
//! the tests, driven by `VELOCITY_SECTOR_SCALE_REPO`): 2,982 source files,
//! 39.8 MB, 951,182 lines decompose in about three seconds into 1,175 sectors,
//! 3,854 dispatches and 260 dependency waves. At the crucible's observed
//! 300 s per dispatch that is 321 model-hours, or roughly 40 wall-hours at
//! eight-way parallelism — arithmetic this module reports rather than hides,
//! because it is the number that decides whether an unattended rewrite of a
//! codebase this size is a thing anybody should start.
//!
//! Three honest limitations are recorded in the output rather than hidden:
//!
//! * Real dependency graphs are not DAGs. Cycles are broken deterministically
//!   (back edges in sorted DFS order) and counted in
//!   [`SectorTotals::cycles_broken`], so the caller can see how much of the
//!   ordering is invented. On LiteLLM that count is 5,764 — which is the truth
//!   about a codebase whose `__init__.py` files re-export everything. Wave
//!   order on such a repo is a *suggestion*, not a guarantee that wave N
//!   compiles against waves 0..N-1.
//! * Wave order says what *should* be ported first. It does not make a
//!   partially-ported crate compile; sectors in the same wave may still need
//!   stubs for each other.
//! * Generated and vendored files are excluded, not ported, and the exclusions
//!   are reported. See [`SkippedGenerated`]. Three rules decide this, each
//!   catching output the others cannot: the filename ([`GENERATED_NAME_SUFFIXES`]),
//!   the line shape of a minified blob ([`GENERATED_AVG_LINE_LEN`]), and the
//!   provenance banner a generator writes into its own output
//!   ([`GENERATED_HEADER_MARKERS`]).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// Directories never worth walking, whatever the language.
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".github",
    ".hg",
    ".svn",
    "node_modules",
    "target",
    "__pycache__",
    ".venv",
    "venv",
    "env",
    ".tox",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    "dist",
    "build",
    ".next",
    // Next.js's emitted asset tree. Nothing is ever hand-written into `_next/`,
    // and LiteLLM commits 311 files of it under
    // `litellm/proxy/_experimental/out/_next/static/` — chunked JS bundles that
    // were being scheduled as portable source until this entry existed.
    "_next",
    "vendor",
    "site-packages",
];

/// Directory names that hold no shippable source even when they hold files.
///
/// Matched as a bare component name at *any* depth, so the rule for membership
/// is strict: a name belongs here only if no real library would give its own
/// package that name. `docs`, `examples`, `scripts`, `migrations` and `benchmarks`
/// are maintainer conventions. `packaging` is not, and it was in this list:
/// pypa/packaging keeps its entire library at `src/packaging/`, so the collector
/// discarded all 480 KB of it before root detection ever ran, and the plan then
/// nominated `tasks/` — 16 KB of nox helpers — as the package root of a library
/// it had never seen. Removing the entry costs the repo it was added for nothing,
/// since `litellm/packaging/` holds a Homebrew formula, a README and a
/// `pyproject.toml`, none of which are portable source in any language
/// [`lang_for_ext`] recognises.
///
/// Note what this list does *not* cover: a skip here happens in the collector,
/// upstream of [`SourceRootDecision`], so bytes dropped by a directory-name match
/// are not part of that decision's coverage arithmetic. The list is therefore the
/// one place in this module where an entry can still remove code without leaving
/// a trace in the plan, which is exactly why membership is the strict rule above
/// rather than "directories that are usually not interesting".
const NON_SOURCE_DIRS: &[&str] = &[
    "docs",
    "doc",
    "examples",
    "example",
    "cookbook",
    "samples",
    "benchmarks",
    "benchmark",
    "helm",
    "terraform",
    "docker",
    "migrations",
    "scripts",
    "ci_cd",
    ".circleci",
    ".devcontainer",
    ".githooks",
    ".semgrep",
    ".greptile",
    ".cargo",
];

/// The share of collected source a directory must hold before it is treated as
/// the package root at all. Below this it is one of several peer directories,
/// and nominating it would be an arbitrary pick dressed up as a detection.
const ROOT_DOMINANCE_FLOOR: f64 = 0.25;

/// The share a nominated root must actually *cover* before the plan scopes to
/// it.
///
/// Dominance and coverage are different tests and the old code only applied the
/// first, which is how a 16 KB directory became the root of a 480 KB library:
/// the exclusions had already removed the library from the pool, so the
/// survivor was dominant over a denominator that no longer meant anything.
/// Three quarters is a round number chosen for the asymmetry of the two errors —
/// declining a real root costs longer sector ids and an output tree that keeps
/// the `src/` prefix, while adopting a wrong one silently drops code the
/// mission then reports as complete.
const ROOT_COVERAGE_FLOOR: f64 = 0.75;

/// What nominating a package root costs, measured rather than assumed.
///
/// Scoping exists to shorten sector ids and make dependency paths resolvable
/// against one root. It is a convenience, so the numbers here are what stop it
/// becoming a silent scope reduction: a plan that scheduled 16 KB of a 496 KB
/// tree used to look identical to one that scheduled all of it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SourceRootDecision {
    /// The root adopted, relative to the repo root. Empty means the repo root
    /// is the scope, so everything collected is scheduled and nothing below is
    /// dropped.
    pub root: String,
    pub inside_files: usize,
    pub inside_bytes: u64,
    /// Collected source that falls outside [`Self::root`] and so is not in the
    /// plan. Always zero when `root` is empty.
    pub outside_files: usize,
    pub outside_bytes: u64,
    /// Up to five of the dropped paths, because "480 KB excluded" is not
    /// actionable and "src/packaging/__init__.py excluded" is.
    pub outside_examples: Vec<String>,
    /// Set when a directory was the largest single holder of source but was
    /// deliberately *not* adopted, naming it and giving the arithmetic. Empty
    /// when no candidate was close enough to be worth mentioning.
    pub declined: String,
}

impl SourceRootDecision {
    /// Fraction of collected source bytes the plan will actually schedule.
    pub fn coverage(&self) -> f64 {
        let total = self.inside_bytes + self.outside_bytes;
        if total == 0 {
            return 1.0;
        }
        self.inside_bytes as f64 / total as f64
    }
}

/// Path components marking a file as test code.
const TEST_MARKERS: &[&str] = &["tests", "test", "testing", "spec", "specs", "e2e"];

/// Filename shapes that mean "build output" whatever directory they sit in.
///
/// Cheap insurance behind [`SKIP_DIRS`]: a project that commits its bundle to
/// the repo root instead of under `dist/` is not hypothetical, and reading a
/// file to discover it is minified costs up to an 8 MB read to learn what the
/// name already said. Source maps and lockfiles are absent because
/// [`lang_for_ext`](crate::registry::team_tools::lang_for_ext) does not accept
/// `.map`, `.json` or `.lock`, so those never reach this check.
///
/// `.d.ts` is deliberately *not* here even though the largest generated file
/// this module has met is one (LiteLLM's `schema.d.ts`, 3 MB, emitted by
/// `openapi-typescript`): a hand-written declaration file is real source, and
/// the name cannot tell the two apart. [`GENERATED_HEADER_MARKERS`] can, from
/// the banner the generator wrote into the file itself.
const GENERATED_NAME_SUFFIXES: &[&str] = &[
    ".min.js",
    ".min.mjs",
    ".min.cjs",
    ".min.css",
    ".bundle.js",
    ".bundle.css",
];

/// An *average* line longer than this means the file was emitted by a tool, not
/// written by a person.
///
/// Average, not maximum: the obvious rule ("any line over N characters is
/// minified") also drops ordinary hand-written source that happens to contain
/// one long string constant or one wide table literal, and silently losing real
/// files from a port is much worse than keeping a vendored bundle. LiteLLM's
/// `swagger-ui-bundle.js` averages 725,000 characters per line; a 12 KB source
/// file with one 9 KB line in it averages 30.
const GENERATED_AVG_LINE_LEN: u64 = 300;

/// How much of a large file is read to judge its shape. Enough to see the line
/// structure without pulling a 100 MB blob into memory to decide it is
/// generated.
const MEASURE_CAP: u64 = 8 * 1024 * 1024;

/// How much of a file is read to look for a generated-content banner.
///
/// Banners are a header convention, so the first half-kilobyte is where they
/// live; anything deeper is a comment about the code rather than about the
/// file's provenance. 512 bytes covers a licence block plus a banner in every
/// generator this has been checked against.
const HEADER_SNIFF_BYTES: usize = 512;

/// Header text, matched case-insensitively, that means "a tool emitted this".
///
/// The third generated-content rule, and the only one that works on output a
/// person can read. [`GENERATED_NAME_SUFFIXES`] needs the name to say so and
/// [`GENERATED_AVG_LINE_LEN`] needs the file to be minified; neither fires on
/// LiteLLM's `ui/litellm-dashboard/src/lib/http/schema.d.ts` — 3 MB of
/// perfectly legible TypeScript declarations whose own first lines read
/// "This file was auto-generated by openapi-typescript. / Do not make direct
/// changes to the file." That file was being scheduled as 128 porting
/// dispatches, roughly 10.7 model-hours and the single largest sector in the
/// plan, to hand-translate a type surface that is regenerated from an OpenAPI
/// spec on demand.
///
/// Membership rule: a phrase belongs here only if it is a codegen convention
/// *about the file carrying it*, not something a person would write about
/// somebody else's output. The set covers Buck/Thrift (`@generated`), the
/// `auto-generated`/`automatically generated` phrasing that
/// openapi-typescript, swagger-codegen, graphql-codegen and OpenAPI Generator
/// all emit, and the `do not edit` half of Go's and protoc's
/// `Code generated by <tool>. DO NOT EDIT.`
///
/// `"generated by"` on its own is *not* here, and that is a measured result
/// rather than caution. The first real repository this ran against excluded two
/// hand-written files on it, both in `litellm-rust/crates/python-compat/src/`:
/// `lib.rs`, whose crate-level doc reads "its tests replay fixtures generated
/// by that operation in CPython", and `repr.rs`, which carries
/// "Generated by `scripts/generate_nonprintable.py`; see
/// `generated/nonprintable.rs`" as the doc comment on a `mod` that *includes*
/// that generated file. Both sentences describe a different artifact than the
/// one they are written in, and both files are real source — `repr.rs` is 238
/// lines of hand-written Rust. `"code generated by"` keeps the Go and protoc
/// banners, where the subject of the sentence is the file itself, without
/// matching prose that merely mentions a generator. A false positive here is
/// the worse error by the same argument as [`GENERATED_AVG_LINE_LEN`]: it
/// removes real code from a port, and it does so quietly enough that the plan
/// still reports success.
///
/// Every exclusion is reported through [`SkippedGenerated::examples`] with the
/// banner text that caused it, so a rule that starts eating real source is
/// diagnosable rather than invisible.
const GENERATED_HEADER_MARKERS: &[&str] = &[
    "@generated",
    "auto-generated",
    "auto generated",
    "autogenerated",
    "automatically generated",
    "code generated by",
    "do not edit",
    "do not make direct changes",
];

/// Default parallelism assumed by the wall-clock estimate.
///
/// Mirrors `DEFAULT_MAX_CONCURRENT_WORKERS` in the editor's app state, which is
/// the gate a real mission actually runs against. Deliberately not imported
/// from there: that constant lives in the GUI layer, and a library planning
/// function must not depend on the frontend to produce an estimate.
pub const DEFAULT_PARALLELISM: usize = 8;

/// What a large file actually looks like.
struct FileShape {
    lines: u32,
    /// Bytes read divided by line count.
    avg_line_len: u64,
}

/// Files excluded as generated, minified or vendored.
///
/// Reported instead of dropped silently: "we ported 5,769 files" is a claim the
/// caller cannot check unless the exclusions are visible.
///
/// Covers only files judged individually — by name, by header banner or by line
/// shape. Whole directories in [`SKIP_DIRS`] — `_next/`, `node_modules/`,
/// `target/` — are never entered and so never counted here; enumerating them
/// would report hundreds of thousands of files nobody asked about.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkippedGenerated {
    pub files: usize,
    pub bytes: u64,
    /// Up to five examples, each carrying the reason it was excluded, so a
    /// wrong exclusion is diagnosable rather than merely counted.
    pub examples: Vec<String>,
}

/// Tuning for [`plan_sectors`].
#[derive(Debug, Clone)]
pub struct SectorOptions {
    /// Target ceiling for one sector's source bytes. A sector is one unit of
    /// ownership for an agent, so this is really "how much source fits in a
    /// prompt alongside the instructions and still leaves room to answer".
    pub max_sector_bytes: u64,
    /// A sector with 400 tiny files is not one task either, even under the byte
    /// ceiling — each file is a thing to open, port and cross-check.
    pub max_files_per_sector: usize,
    /// Files above this are split into line-range chunks. Anything bigger than
    /// this cannot be handed to a model whole: LiteLLM's `router.py` is 722 KB.
    pub max_unit_bytes: u64,
    /// Include test files in the decomposition. Off by default — test parity is
    /// its own phase, and folding the suite into the porting sectors inflates
    /// every estimate without changing what gets ported first.
    pub include_tests: bool,
    /// Assumed wall-clock seconds per dispatch, used only for the effort
    /// estimate. The LiteLLM crucible measured 297s for a 7 KB module.
    pub per_dispatch_secs: u64,
    /// Package root to scope the decomposition to, relative to the repo root.
    /// Empty means detect one, which is what almost every caller wants.
    ///
    /// This is the deliberate escape hatch from [`decide_source_root`]'s
    /// coverage guard. A monorepo whose largest directory holds less than
    /// [`ROOT_COVERAGE_FLOOR`] of the source is planned whole rather than
    /// silently narrowed, because the guard cannot know whether the other
    /// directories are separate sub-projects or part of the same library. An
    /// operator who does know says so here, and the choice is then recorded in
    /// the plan instead of being inferred from byte counts.
    pub source_root: String,
}

impl Default for SectorOptions {
    fn default() -> Self {
        Self {
            max_sector_bytes: 48_000,
            max_files_per_sector: 40,
            max_unit_bytes: 24_000,
            include_tests: false,
            per_dispatch_secs: 300,
            source_root: String::new(),
        }
    }
}

/// One source file, relative to the repository root with `/` separators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    pub rel: String,
    pub bytes: u64,
    pub lines: u32,
    pub language: String,
}

/// A dispatchable slice of one file. `chunks == 1` means the whole file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectorUnit {
    pub rel: String,
    pub bytes: u64,
    /// 1-based inclusive first line of this chunk.
    pub line_start: u32,
    /// 1-based inclusive last line of this chunk.
    pub line_end: u32,
    /// Which chunk this is, 1-based.
    pub chunk: u32,
    /// Total chunks for this file.
    pub chunks: u32,
}

impl SectorUnit {
    /// True when this unit is a slice of a larger file rather than all of it.
    pub fn is_partial(&self) -> bool {
        self.chunks > 1
    }

    /// A `file.rs:10-40` style reference for prompts and reports.
    pub fn span_label(&self) -> String {
        if self.is_partial() {
            format!("{}:{}-{}", self.rel, self.line_start, self.line_end)
        } else {
            self.rel.clone()
        }
    }
}

/// A group of source files one agent can own end to end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sector {
    /// Stable slug, unique within a plan. Used as the phase identity.
    pub id: String,
    /// Directory prefix this sector covers, relative to the repo root.
    pub path: String,
    /// Member files, relative to the repo root, sorted.
    pub files: Vec<String>,
    pub bytes: u64,
    pub lines: u32,
    /// Dispatchable units — `files` with oversized entries chunked.
    pub units: Vec<SectorUnit>,
    /// Sector ids this one imports from.
    pub depends_on: Vec<String>,
    /// Set when the sector still holds a unit larger than
    /// [`SectorOptions::max_unit_bytes`] after chunking — i.e. a single line too
    /// long to split. This is *not* "the sector is big": a 890 KB file chunked
    /// into 38 units that each fit a context window is perfectly dispatchable and
    /// reports `oversized == false`. What an operator needs warned about is the
    /// case where one unit still cannot be handed to a model whole.
    pub oversized: bool,
}

/// Repo-wide numbers, including the ones that make the plan look bad.
///
/// `PartialEq` only: the wall-clock estimate is an `f64`, so `Eq` would be a
/// lie about a type that plans are compared with.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SectorTotals {
    pub files: usize,
    pub bytes: u64,
    pub lines: u32,
    pub sectors: usize,
    /// Sum of `units.len()` — i.e. the number of dispatches the port implies.
    pub dispatches: usize,
    pub waves: usize,
    pub largest_wave: usize,
    pub sectors_over_budget: usize,
    pub files_chunked: usize,
    pub chunks: usize,
    /// Dependency edges dropped to break a cycle. A high count means the wave
    /// ordering is largely invented, not discovered.
    pub cycles_broken: usize,
    /// Units that still exceed `max_unit_bytes` after chunking — a single line
    /// too long to split. These cannot be handed to a model whole.
    pub units_over_ceiling: usize,
    /// Files excluded as generated, minified or vendored.
    pub generated_skipped: usize,
    pub generated_bytes: u64,
    /// Up to five excluded files, so a wrong exclusion is diagnosable.
    pub generated_examples: Vec<String>,
    /// Collected source that lies outside the adopted package root and is
    /// therefore **not in the plan**. Zero unless a root was adopted, since the
    /// fallback scope is the repo root. Reported rather than merely avoided
    /// because a plan that quietly scheduled 16 KB of a 496 KB tree used to be
    /// indistinguishable from one that scheduled all of it.
    pub outside_root_files: usize,
    pub outside_root_bytes: u64,
    /// Up to five of them, for the same reason as [`Self::generated_examples`].
    pub outside_root_examples: Vec<String>,
    /// Set when the largest source directory was found but deliberately not
    /// adopted as the root, with the arithmetic that decided it.
    pub root_declined: String,
    /// Estimated model-seconds: dispatches x per-dispatch wall clock.
    pub est_model_secs: u64,
    /// Estimated wall-clock hours at the given parallelism.
    pub est_wall_hours_at_parallelism: f64,
    /// The parallelism the estimate above assumed.
    pub parallelism: usize,
}

impl SectorTotals {
    /// Fraction of collected source bytes this plan actually schedules, 0..=1.
    ///
    /// Derived rather than stored: it is a pure function of `bytes` and
    /// `outside_root_bytes`, and a stored copy would have a `Default` of 0.0 —
    /// which reads as "this plan scheduled nothing" and is exactly the false
    /// alarm a hand-built totals fixture would then raise. Read this before
    /// trusting `bytes`: it says whether `bytes` is the repository or a slice
    /// of it.
    pub fn root_coverage(&self) -> f64 {
        let total = self.bytes + self.outside_root_bytes;
        if total == 0 {
            return 1.0;
        }
        self.bytes as f64 / total as f64
    }

    /// A one-line verdict on whether this decomposition is worth starting.
    ///
    /// Exists because the raw numbers do not communicate the decision. On
    /// LiteLLM they read `est_model_secs: 2049300`, which looks like progress;
    /// it is 569 model-hours, and an operator who has to authorise the spend
    /// should be told that in words rather than left to do the division. The
    /// estimate also excludes retries, rework and the sweep phases, so every
    /// band says so rather than presenting itself as a total.
    pub fn feasibility(&self) -> String {
        let model_hours = self.est_model_secs as f64 / 3600.0;
        let wall = self.est_wall_hours_at_parallelism;
        let spend = format!(
            "{} dispatches, ~{model_hours:.0} model-hours, ~{wall:.0} wall-hours at {}x parallelism (porting only — excludes retries, rework and the sweep phases)",
            self.dispatches, self.parallelism
        );
        // Prefixed rather than appended: the cost estimate is only meaningful
        // once the reader knows what it covers. "321 model-hours" for a whole
        // repository and for a twentieth of one are different claims wearing
        // the same sentence, and the estimate is the number an operator
        // authorises spend against.
        let scope = if self.root_coverage() < 1.0 {
            format!(
                "SCOPE WARNING: this plan covers {:.0}% of the source found — {} files ({} KB) \
                 sit outside the package root and will not be ported. {}\n",
                self.root_coverage() * 100.0,
                self.outside_root_files,
                self.outside_root_bytes / 1024,
                if self.outside_root_examples.is_empty() {
                    String::new()
                } else {
                    format!("Not scheduled: {}.", self.outside_root_examples.join(", "))
                }
            )
        } else {
            String::new()
        };
        // Separate `if`, not `else if`: a declined root and a partial one can
        // both be true of the same plan, and either fact on its own is
        // misleading about the scope.
        let scope = if self.root_declined.is_empty() {
            scope
        } else {
            format!("{scope}SCOPE: {}\n", self.root_declined)
        };
        let verdict = if wall <= 1.0 {
            format!("{spend}: runnable in one sitting.")
        } else if wall <= 8.0 {
            format!("{spend}: an unattended run over roughly a working day. Set a spend cap and checkpoint each wave.")
        } else if wall <= 72.0 {
            format!("{spend}: a multi-day unattended run. Only start this with a hard budget and a resumable executor.")
        } else {
            format!("{spend}: not one mission. Split the repository into independently shippable sub-projects and port them separately, or narrow the scope — no budget survives this.")
        };
        format!("{scope}{verdict}")
    }
}

/// The whole decomposition.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SectorPlan {
    pub repo_name: String,
    /// Detected package root, relative to the repo root (`"litellm"`). Empty
    /// when the tree is flat and the root itself is the source root.
    pub source_root: String,
    pub sectors: Vec<Sector>,
    /// Sector ids grouped by dependency depth; wave 0 has no in-plan deps.
    pub waves: Vec<Vec<String>>,
    pub totals: SectorTotals,
}

impl SectorPlan {
    pub fn sector(&self, id: &str) -> Option<&Sector> {
        self.sectors.iter().find(|s| s.id == id)
    }

    /// Serialise to the JSON shape the mission runner and the plan builder use.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "repo_name": self.repo_name,
            "source_root": self.source_root,
            "totals": serde_json::json!({
                "files": self.totals.files,
                "bytes": self.totals.bytes,
                "lines": self.totals.lines,
                "sectors": self.totals.sectors,
                "dispatches": self.totals.dispatches,
                "waves": self.totals.waves,
                "largest_wave": self.totals.largest_wave,
                "sectors_over_budget": self.totals.sectors_over_budget,
                "files_chunked": self.totals.files_chunked,
                "chunks": self.totals.chunks,
                "cycles_broken": self.totals.cycles_broken,
                "units_over_ceiling": self.totals.units_over_ceiling,
                "generated_skipped": self.totals.generated_skipped,
                "generated_bytes": self.totals.generated_bytes,
                "generated_examples": self.totals.generated_examples,
                "outside_root_files": self.totals.outside_root_files,
                "outside_root_bytes": self.totals.outside_root_bytes,
                "outside_root_examples": self.totals.outside_root_examples,
                "root_declined": self.totals.root_declined,
                "root_coverage": self.totals.root_coverage(),
                "est_model_secs": self.totals.est_model_secs,
                "est_wall_hours_at_parallelism": self.totals.est_wall_hours_at_parallelism,
                "parallelism": self.totals.parallelism,
            }),
            "waves": self.waves,
            "sectors": self.sectors.iter().map(|s| serde_json::json!({
                "id": s.id,
                "path": s.path,
                "files": s.files,
                "bytes": s.bytes,
                "lines": s.lines,
                "depends_on": s.depends_on,
                "oversized": s.oversized,
                "units": s.units.iter().map(|u| serde_json::json!({
                    "rel": u.rel,
                    "bytes": u.bytes,
                    "line_start": u.line_start,
                    "line_end": u.line_end,
                    "chunk": u.chunk,
                    "chunks": u.chunks,
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    }

    /// Inverse of [`to_json`], for a resumed mission reading back the plan its
    /// sectors were started from.
    ///
    /// Re-deriving instead would be *almost* as good and wrong in the case that
    /// matters: decomposition walks the clone, and a mission that runs for
    /// hours can be resumed against a clone somebody has since updated. Sector
    /// ids would shift, `sectors_done` would stop matching, and the runner
    /// would quietly re-port work it had already paid for. Reading the
    /// persisted plan makes the ids stable by construction.
    ///
    /// Errors rather than defaulting: a truncated or hand-edited plan file is
    /// not something to guess at, because every downstream decision (which
    /// sectors remain, what the budget should be) is derived from it.
    pub fn from_json(v: &serde_json::Value) -> Result<Self, String> {
        let req = |k: &str| -> Result<&serde_json::Value, String> {
            v.get(k)
                .ok_or_else(|| format!("sector plan is missing '{k}'"))
        };

        let t = req("totals")?;
        let totals = SectorTotals {
            files: usz_v(t, "files")?,
            bytes: u64_v(t, "bytes")?,
            lines: u32_v(t, "lines")?,
            sectors: usz_v(t, "sectors")?,
            dispatches: usz_v(t, "dispatches")?,
            waves: usz_v(t, "waves")?,
            largest_wave: usz_v(t, "largest_wave")?,
            sectors_over_budget: usz_v(t, "sectors_over_budget")?,
            files_chunked: usz_v(t, "files_chunked")?,
            chunks: usz_v(t, "chunks")?,
            cycles_broken: usz_v(t, "cycles_broken")?,
            units_over_ceiling: usz_v(t, "units_over_ceiling")?,
            generated_skipped: usz_v(t, "generated_skipped")?,
            generated_bytes: u64_v(t, "generated_bytes")?,
            generated_examples: strs_v(t, "generated_examples")?,
            // Lenient, unlike the fields above: these were added after plans
            // started being persisted, and refusing to resume a mission over a
            // reporting field would strand hours of paid-for work. An absent
            // value means "written by an older build", so coverage defaults to
            // total rather than zero — claiming the old plan dropped code it may
            // well have scheduled would be as false as claiming it dropped none.
            outside_root_files: t
                .get("outside_root_files")
                .and_then(|x| x.as_u64())
                .unwrap_or(0) as usize,
            outside_root_bytes: t
                .get("outside_root_bytes")
                .and_then(|x| x.as_u64())
                .unwrap_or(0),
            outside_root_examples: t
                .get("outside_root_examples")
                .map(|x| {
                    x.as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|y| y.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default()
                })
                .unwrap_or_default(),
            root_declined: t
                .get("root_declined")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            est_model_secs: u64_v(t, "est_model_secs")?,
            est_wall_hours_at_parallelism: f64_v(t, "est_wall_hours_at_parallelism")?,
            parallelism: usz_v(t, "parallelism")?,
        };

        let waves_raw = req("waves")?
            .as_array()
            .ok_or("sector plan field 'waves' is not an array")?;
        let mut waves = Vec::with_capacity(waves_raw.len());
        for w in waves_raw {
            waves.push(
                w.as_array()
                    .ok_or("a wave in the sector plan is not an array")?
                    .iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect(),
            );
        }

        let sectors_raw = req("sectors")?
            .as_array()
            .ok_or("sector plan field 'sectors' is not an array")?;
        let mut sectors = Vec::with_capacity(sectors_raw.len());
        for sv in sectors_raw {
            let units_raw = sv
                .get("units")
                .and_then(|u| u.as_array())
                .ok_or("a sector in the plan has no 'units' array")?;
            let units = units_raw
                .iter()
                .map(|uv| {
                    Ok(SectorUnit {
                        rel: s_v(uv, "rel")?,
                        bytes: u64_v(uv, "bytes")?,
                        line_start: u32_v(uv, "line_start")?,
                        line_end: u32_v(uv, "line_end")?,
                        chunk: u32_v(uv, "chunk")?,
                        chunks: u32_v(uv, "chunks")?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            // Absent means an older plan file: `oversized` is derived from the
            // units, so recompute it rather than defaulting to `false` and
            // hiding a unit no model can be handed whole.
            let oversized = sv
                .get("oversized")
                .and_then(|o| o.as_bool())
                .unwrap_or(false);
            sectors.push(Sector {
                id: s_v(sv, "id")?,
                path: s_v(sv, "path")?,
                files: strs_v(sv, "files")?,
                bytes: u64_v(sv, "bytes")?,
                lines: u32_v(sv, "lines")?,
                depends_on: strs_v(sv, "depends_on")?,
                oversized,
                units,
            });
        }

        Ok(SectorPlan {
            repo_name: s_v(v, "repo_name")?,
            source_root: s_v(v, "source_root")?,
            sectors,
            waves,
            totals,
        })
    }
}

// Field readers, each naming the key it failed on. `from_json` is hand-written
// rather than a `Deserialize` derive because `SectorPlan` is constructed
// field-by-field in `plan_sectors_scoped` and a derive would oblige every
// future field to be optional-or-defaulted at the definition site; here a
// missing key is an error, which is what a truncated plan file should produce.
fn s_v(v: &serde_json::Value, k: &str) -> Result<String, String> {
    v.get(k)
        .and_then(|x| x.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("sector plan field '{k}' is missing or not a string"))
}

fn u64_v(v: &serde_json::Value, k: &str) -> Result<u64, String> {
    v.get(k)
        .and_then(|x| x.as_u64())
        .ok_or_else(|| format!("sector plan field '{k}' is missing or not an integer"))
}

fn u32_v(v: &serde_json::Value, k: &str) -> Result<u32, String> {
    v.get(k)
        .and_then(|x| x.as_u64())
        .map(|n| n as u32)
        .ok_or_else(|| format!("sector plan field '{k}' is missing or not an integer"))
}

fn usz_v(v: &serde_json::Value, k: &str) -> Result<usize, String> {
    v.get(k)
        .and_then(|x| x.as_u64())
        .map(|n| n as usize)
        .ok_or_else(|| format!("sector plan field '{k}' is missing or not an integer"))
}

fn f64_v(v: &serde_json::Value, k: &str) -> Result<f64, String> {
    v.get(k)
        .and_then(|x| x.as_f64())
        .ok_or_else(|| format!("sector plan field '{k}' is missing or not a number"))
}

fn strs_v(v: &serde_json::Value, k: &str) -> Result<Vec<String>, String> {
    v.get(k)
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .ok_or_else(|| format!("sector plan field '{k}' is missing or not an array"))
}

/// Walk `repo_root` and collect every source file worth porting.
///
/// Deterministic: directory entries are sorted before use, because `read_dir`
/// order is a filesystem property and a plan that changes between runs cannot
/// be resumed against.
pub fn collect_source_files(repo_root: &Path, opts: &SectorOptions) -> Vec<SourceFile> {
    collect_source_files_reported(repo_root, opts).0
}

/// [`collect_source_files`] plus what was excluded as generated, and why.
pub fn collect_source_files_reported(
    repo_root: &Path,
    opts: &SectorOptions,
) -> (Vec<SourceFile>, SkippedGenerated) {
    let mut out = Vec::new();
    let mut skipped = SkippedGenerated::default();
    let mut stack: Vec<(PathBuf, usize)> = vec![(repo_root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > 12 {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if SKIP_DIRS.contains(&name.as_str()) || NON_SOURCE_DIRS.contains(&name.as_str()) {
                    continue;
                }
                stack.push((path, depth + 1));
                continue;
            }
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            let Some(language) =
                crate::registry::team_tools::lang_for_ext(&ext.to_ascii_lowercase())
            else {
                continue;
            };
            let Ok(rel_path) = path.strip_prefix(repo_root) else {
                continue;
            };
            let rel = rel_path
                .components()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join("/");
            if !opts.include_tests && is_test_path(&rel) {
                continue;
            }
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            if generated_by_name(&name) {
                skip_generated(
                    &mut skipped,
                    &rel,
                    meta.len(),
                    "generated filename".to_string(),
                );
                continue;
            }
            // One open per file whichever size it is: a small file reads only
            // the header it takes to find a banner, a large file reads the
            // [`MEASURE_CAP`] the line-shape check already needed and the
            // banner comes out of the same buffer. Small files are opened at
            // all — collection used to be stat-only for them — because the
            // header rule does not depend on size, and a rule with a size hole
            // in it is a rule that gets rediscovered later by somebody
            // measuring a repository full of small generated files.
            let large = meta.len() > opts.max_unit_bytes;
            let head = read_head(
                &path,
                if large {
                    MEASURE_CAP as usize
                } else {
                    HEADER_SNIFF_BYTES
                },
            );
            if let Some(banner) = head.as_deref().and_then(generated_banner) {
                skip_generated(&mut skipped, &rel, meta.len(), format!("header: {banner}"));
                continue;
            }
            // Counting newlines is only worth it for a file big enough to be
            // chunked; the effort estimate does not need exact lines for the
            // rest, and guessing from bytes keeps them at stat-only cost.
            let lines = if !large {
                ((meta.len() / 40).max(1)) as u32
            } else {
                match head.as_deref().map(shape_of) {
                    Some(shape) if shape.avg_line_len > GENERATED_AVG_LINE_LEN => {
                        // Generated, minified or vendored. Two reasons to drop
                        // it, either sufficient: porting somebody else's build
                        // output is meaningless, and a file with no line
                        // structure cannot be chunked into units that fit a
                        // context window.
                        skip_generated(
                            &mut skipped,
                            &rel,
                            meta.len(),
                            format!(
                                "{} lines, avg line {} chars",
                                shape.lines, shape.avg_line_len
                            ),
                        );
                        continue;
                    }
                    Some(shape) => shape.lines,
                    // Unreadable: assume hand-written rather than exclude. The
                    // porting dispatch will hit the read failure and report it,
                    // which is where a broken file belongs — silently leaving it
                    // out of the plan would hide it behind a success.
                    None => ((meta.len() / 40).max(1)) as u32,
                }
            };
            out.push(SourceFile {
                rel,
                bytes: meta.len(),
                lines,
                language: language.to_string(),
            });
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    (out, skipped)
}

/// True when the filename alone says the file is build output.
fn generated_by_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    GENERATED_NAME_SUFFIXES
        .iter()
        .any(|suffix| lower.ends_with(suffix))
}

/// True when any path component marks the file as test code.
fn is_test_path(rel: &str) -> bool {
    rel.split('/').any(|part| {
        TEST_MARKERS.contains(&part)
            || part.starts_with("test_")
            || part.ends_with("_test.py")
            || part.ends_with("_test.rs")
            || part.ends_with(".test.ts")
            || part.ends_with(".test.js")
            || part.ends_with(".spec.ts")
            || part.ends_with(".spec.js")
    })
}

/// Read up to `cap` bytes from the start of `path`.
///
/// `None` only when the file cannot be read at all, which the caller treats as
/// "assume hand-written" rather than "exclude" — an unreadable file is a problem
/// the porting dispatch will hit and report, not a reason to silently drop it
/// from the plan.
fn read_head(path: &Path, cap: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; cap];
    let read = file.read(&mut buf).ok()?;
    buf.truncate(read);
    Some(buf)
}

/// Line count and average line length of a buffer holding a file's first bytes.
fn shape_of(buf: &[u8]) -> FileShape {
    let newlines = buf.iter().filter(|&&b| b == b'\n').count() as u32;
    let lines = if buf.last() == Some(&b'\n') {
        newlines.max(1)
    } else {
        newlines + 1
    };
    FileShape {
        lines,
        avg_line_len: (buf.len() as u64) / u64::from(lines.max(1)),
    }
}

/// The banner line that identifies `head` as generated output, if there is one.
///
/// Only the first [`HEADER_SNIFF_BYTES`] are examined even when `head` holds
/// megabytes: a provenance banner is a header convention, and decoding an 8 MB
/// buffer to search it would cost a full UTF-8 validation pass per large file
/// to learn something stated in its first line.
fn generated_banner(head: &[u8]) -> Option<String> {
    let window = &head[..head.len().min(HEADER_SNIFF_BYTES)];
    let text = String::from_utf8_lossy(window);
    let line = text.lines().find(|line| {
        let lower = line.to_ascii_lowercase();
        GENERATED_HEADER_MARKERS.iter().any(|m| lower.contains(m))
    })?;
    // Strip the comment furniture the banner is wrapped in, so what reaches
    // `generated_examples` is the sentence the generator wrote rather than a
    // run of `///` or `*` around it. Trimming cannot empty a line that matched
    // a marker, since every marker contains an ASCII letter.
    let cleaned = line
        .trim_matches(|c: char| {
            c.is_whitespace() || matches!(c, '/' | '*' | '#' | '-' | '<' | '>' | '!' | ';')
        })
        .trim();
    // Truncate on a char boundary: a banner may contain non-ASCII and slicing
    // by byte index would panic on a repository nobody predicted.
    let mut end = cleaned.len().min(120);
    while !cleaned.is_char_boundary(end) {
        end -= 1;
    }
    Some(if end < cleaned.len() {
        format!("{}…", &cleaned[..end])
    } else {
        cleaned.to_string()
    })
}

/// Record one file excluded as generated, and why.
///
/// Three independent rules can exclude a file — name, header banner and line
/// shape — so the bookkeeping lives here rather than at each of the three
/// sites. The counts have to agree about what was dropped, and the reason has
/// to survive into [`SkippedGenerated::examples`]: that list is the only way an
/// operator can tell a correct exclusion from a rule that has started eating
/// real source.
fn skip_generated(skipped: &mut SkippedGenerated, rel: &str, bytes: u64, why: String) {
    skipped.files += 1;
    skipped.bytes += bytes;
    if skipped.examples.len() < 5 {
        skipped
            .examples
            .push(format!("{rel} ({} KB, {why})", bytes / 1024));
    }
}

/// Number of lines in a file, by the usual convention: a trailing newline
/// terminates the last line rather than starting an empty one.
///
/// Used by both the collector's measurement and the chunker's spans. Two
/// conventions here would make [`SourceFile::lines`] disagree with the last
/// chunk's `line_end` — the sort of off-by-one that surfaces much later as a
/// ported file silently missing its tail.
fn line_count(text: &str) -> u32 {
    let newlines = text.bytes().filter(|&b| b == b'\n').count() as u32;
    if text.ends_with('\n') {
        newlines.max(1)
    } else {
        newlines + 1
    }
}

/// Account for what scoping to `root` costs, without judging whether the scope
/// is a good idea.
///
/// This is the shared half of [`decide_source_root`] and the whole of an
/// operator-supplied root, where the judgement has already been made by somebody
/// who can see the repository.
fn measure_against_root(files: &[SourceFile], root: &str) -> SourceRootDecision {
    let root = root.trim_matches('/');
    let total: u64 = files.iter().map(|f| f.bytes).sum();
    if root.is_empty() {
        // The repo root as scope: everything collected is scheduled, so
        // coverage is total by construction and there is nothing to account for.
        return SourceRootDecision {
            root: String::new(),
            inside_files: files.len(),
            inside_bytes: total,
            ..Default::default()
        };
    }
    let prefix = format!("{root}/");
    let mut d = SourceRootDecision {
        root: root.to_string(),
        ..Default::default()
    };
    for f in files {
        if f.rel.starts_with(&prefix) {
            d.inside_files += 1;
            d.inside_bytes += f.bytes;
        } else {
            d.outside_files += 1;
            d.outside_bytes += f.bytes;
            if d.outside_examples.len() < 5 {
                d.outside_examples
                    .push(format!("{} ({} KB)", f.rel, f.bytes / 1024));
            }
        }
    }
    d
}

/// Pick the directory that actually holds the code, and account for what the
/// pick costs.
///
/// A repo root is mostly not source: LiteLLM's holds `helm/`, `ui/`,
/// `cookbook/` and 60 config files around the one `litellm/` package that
/// matters. Choosing the biggest source directory makes every sector id short
/// and every dependency path resolvable against a single package root. Falls
/// back to the repo root when no directory dominates, so flat repos still
/// decompose instead of producing one giant sector.
///
/// Two gates, because one is not enough. *Dominance* — the directory must hold
/// at least [`ROOT_DOMINANCE_FLOOR`] of the collected source, or it is a peer
/// among equals and picking it is arbitrary. *Coverage* — it must also hold at
/// least [`ROOT_COVERAGE_FLOOR`], because everything outside an adopted root is
/// dropped from the plan. The old code applied only the first and measured it
/// against a denominator the collector had already emptied, which is how
/// `tasks/` (16 KB) became the package root of pypa/packaging (480 KB) and the
/// mission reported a complete rewrite of a library it had never scheduled.
pub fn decide_source_root(files: &[SourceFile]) -> SourceRootDecision {
    let mut by_top: BTreeMap<&str, u64> = BTreeMap::new();
    let mut total = 0u64;
    for f in files {
        total += f.bytes;
        if let Some((head, _)) = f.rel.split_once('/') {
            *by_top.entry(head).or_default() += f.bytes;
        }
    }

    // No candidate worth naming: plan the whole tree.
    let whole_tree = || measure_against_root(files, "");

    if total == 0 {
        return whole_tree();
    }
    let Some((best, best_bytes)) = by_top
        .iter()
        .filter(|(name, _)| !NON_SOURCE_DIRS.contains(name) && !name.starts_with('.'))
        .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
        .map(|(name, bytes)| ((*name).to_string(), *bytes))
    else {
        return whole_tree();
    };

    // Dominance: is this directory the codebase, or just its largest room?
    if (best_bytes as f64 / total as f64) < ROOT_DOMINANCE_FLOOR {
        return whole_tree();
    }

    let d = measure_against_root(files, &best);

    // Coverage: adopting it drops the rest. Refuse the drop rather than merely
    // report it — an unattended mission cannot act on a warning it never reads,
    // and "scheduled 3% of the tree, completed successfully" is the failure mode
    // this whole module is supposed to make impossible.
    let cov = d.coverage();
    if cov < ROOT_COVERAGE_FLOOR {
        let mut w = whole_tree();
        w.declined = format!(
            "'{best}/' holds {:.0}% of the collected source — the largest single directory, but \
             scoping to it would leave {} files ({} KB) out of the plan, so the repo root is the \
             scope instead and everything collected is scheduled. Pass 'source_root' to narrow it \
             deliberately if only that directory was wanted.",
            cov * 100.0,
            d.outside_files,
            d.outside_bytes / 1024
        );
        return w;
    }
    d
}

/// Decompose a cloned repository into dependency-ordered sectors.
pub fn plan_sectors(repo_root: &Path, opts: &SectorOptions) -> SectorPlan {
    plan_sectors_with_parallelism(repo_root, opts, 1)
}

/// [`plan_sectors`] with an explicit parallelism for the wall-clock estimate.
///
/// This is the only entry point that registers the repository root, which unit
/// chunking needs to read the oversized files it is splitting. Callers must go
/// through here rather than the helpers below, or chunking silently degrades to
/// one whole-file unit per file.
pub fn plan_sectors_with_parallelism(
    repo_root: &Path,
    opts: &SectorOptions,
    parallelism: usize,
) -> SectorPlan {
    // Save/restore rather than just set: a nested call (a mission planning the
    // next repo while holding state for this one) must not inherit the inner
    // root after the inner call returns.
    let previous = CURRENT_ROOT.with(|cell| cell.borrow_mut().replace(repo_root.to_path_buf()));
    let plan = plan_sectors_inner(repo_root, opts, parallelism);
    CURRENT_ROOT.with(|cell| *cell.borrow_mut() = previous);
    plan
}

/// Alias kept for callers that want to name the parallelism explicitly.
pub fn plan_sectors_scoped(
    repo_root: &Path,
    opts: &SectorOptions,
    parallelism: usize,
) -> SectorPlan {
    plan_sectors_with_parallelism(repo_root, opts, parallelism)
}

fn plan_sectors_inner(repo_root: &Path, opts: &SectorOptions, parallelism: usize) -> SectorPlan {
    let (all, skipped) = collect_source_files_reported(repo_root, opts);
    // An explicit root is the operator's scope decision and is taken as given;
    // only *detection* has to prove that scoping is not costing code.
    let mut decision = if opts.source_root.trim().is_empty() {
        decide_source_root(&all)
    } else {
        measure_against_root(&all, opts.source_root.trim())
    };
    if !decision.root.is_empty() && decision.inside_files == 0 {
        // Respecting a bad path silently would produce an empty plan that
        // reports "completed", which is the failure mode this module exists to
        // prevent, arriving by a different route. The plan stays empty — the
        // operator asked for that scope and widening it would be worse — but it
        // says so in terms nobody can read as success.
        decision.declined = format!(
            "the requested source_root '{}' matched 0 of {} collected source files, so this plan \
             has no sectors and ports nothing. Check the path against the clone; nothing was \
             widened to compensate.",
            decision.root,
            all.len()
        );
    }
    let source_root = decision.root.clone();
    let prefix = if source_root.is_empty() {
        String::new()
    } else {
        format!("{source_root}/")
    };
    let scoped: Vec<&SourceFile> = all
        .iter()
        .filter(|f| prefix.is_empty() || f.rel.starts_with(&prefix))
        .collect();

    let mut sectors: Vec<Sector> = Vec::new();
    split_group(&source_root, &scoped, opts, &mut sectors, 0);
    // Stable identity: same tree in, same ids and same order out.
    sectors.sort_by(|a, b| a.id.cmp(&b.id));
    make_ids_unique(&mut sectors);

    link_dependencies(&source_root, &mut sectors);
    let (waves, cycles_broken) = assign_waves(&mut sectors);

    let repo_name = repo_root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "repo".to_string());

    let mut plan = SectorPlan {
        repo_name,
        source_root,
        waves,
        sectors,
        totals: SectorTotals::default(),
    };
    summarise(
        &mut plan,
        cycles_broken,
        skipped,
        &decision,
        opts,
        parallelism.max(1),
    );
    plan
}

/// Make sector ids unique.
///
/// Slugifying a directory path is lossy — `open_ai/` and `openai/` both become
/// `openai`, and a directory genuinely named `x-root` collides with the
/// loose-file label for `x/`. On a 1,300-sector decomposition of LiteLLM that
/// stopped being theoretical: colliding ids collapsed the dependency map, and
/// sectors vanished from the wave schedule without any error, because a
/// `BTreeMap` keyed by id simply overwrote one with the other.
///
/// Runs after the sort, so the first sector in id order keeps the bare name and
/// later ones get a numeric suffix — deterministic across runs.
fn make_ids_unique(sectors: &mut [Sector]) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for s in sectors.iter_mut() {
        match seen.get(&s.id).copied() {
            None => {
                seen.insert(s.id.clone(), 1);
            }
            Some(n) => {
                let mut candidate = format!("{}-{}", s.id, n + 1);
                while seen.contains_key(&candidate) {
                    seen.insert(candidate.clone(), 1);
                    candidate = format!("{candidate}-x");
                }
                seen.insert(s.id.clone(), n + 1);
                seen.insert(candidate.clone(), 1);
                s.id = candidate;
            }
        }
    }
}

/// Recursively split one directory's files into sectors under the byte and
/// file-count ceilings.
fn split_group(
    dir: &str,
    files: &[&SourceFile],
    opts: &SectorOptions,
    out: &mut Vec<Sector>,
    depth: usize,
) {
    if files.is_empty() {
        return;
    }
    let bytes: u64 = files.iter().map(|f| f.bytes).sum();
    let lines = sum_lines(files.iter().map(|f| f.lines));
    let fits = bytes <= opts.max_sector_bytes && files.len() <= opts.max_files_per_sector;
    if fits || depth >= 12 {
        out.push(make_sector(dir, files, bytes, lines, opts));
        return;
    }

    // Partition on the immediate child directory. Loose files in `dir` itself
    // land in the empty key and are bin-packed below.
    let mut groups: BTreeMap<String, Vec<&SourceFile>> = BTreeMap::new();
    for f in files {
        let rest = strip_prefix_dir(&f.rel, dir);
        let key = match rest.split_once('/') {
            Some((head, _)) => head.to_string(),
            None => String::new(),
        };
        groups.entry(key).or_default().push(*f);
    }

    for (key, members) in groups {
        if key.is_empty() {
            emit_loose_bins(dir, &members, opts, out);
        } else {
            let child = if dir.is_empty() {
                key
            } else {
                format!("{dir}/{key}")
            };
            split_group(&child, &members, opts, out, depth + 1);
        }
    }
}

/// Bin-pack the files sitting directly in `dir` into sectors under the byte and
/// file-count ceilings.
///
/// Greedy in sorted order, which is the point: two runs over the same tree cut
/// the same bins, so a resumed mission recognises the sectors it already did
/// instead of re-porting them under new ids.
fn emit_loose_bins(
    dir: &str,
    members: &[&SourceFile],
    opts: &SectorOptions,
    out: &mut Vec<Sector>,
) {
    let mut part = 1usize;
    let mut bin: Vec<&SourceFile> = Vec::new();
    let mut bin_bytes = 0u64;
    for f in members.iter().copied() {
        let full = !bin.is_empty()
            && (bin_bytes + f.bytes > opts.max_sector_bytes
                || bin.len() >= opts.max_files_per_sector);
        if full {
            flush_bin(dir, part, &bin, opts, out);
            part += 1;
            bin.clear();
            bin_bytes = 0;
        }
        bin.push(f);
        bin_bytes += f.bytes;
    }
    if !bin.is_empty() {
        flush_bin(dir, part, &bin, opts, out);
    }
}

fn flush_bin(
    dir: &str,
    part: usize,
    bin: &[&SourceFile],
    opts: &SectorOptions,
    out: &mut Vec<Sector>,
) {
    let label = loose_label(dir, part);
    let bytes: u64 = bin.iter().map(|x| x.bytes).sum();
    let lines = sum_lines(bin.iter().map(|x| x.lines));
    out.push(make_sector_named(&label, dir, bin, bytes, lines, opts));
}

/// Saturating line total.
///
/// `Iterator::sum` on `u32` panics on overflow in debug builds, and a repo of
/// LiteLLM's size (~1.1M lines) summed across sectors that overlap in the
/// estimate is close enough to the ceiling to be worth not risking.
fn sum_lines(values: impl Iterator<Item = u32>) -> u32 {
    values.fold(0u32, u32::saturating_add)
}

/// Language of a file, from its extension, defaulting to Python.
///
/// The default is a guess, but a harmless one: `imports_of` returns nothing for
/// a language it does not know, so a mislabelled file simply contributes no
/// edges rather than wrong ones.
fn language_of(rel: &str) -> &'static str {
    rel.rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .as_deref()
        .and_then(crate::registry::team_tools::lang_for_ext)
        .unwrap_or("python")
}

/// Sector id for a bin of loose files: `litellm-root`, `litellm-root-2`, ...
///
/// Slugified, because the id is the identity a resumed mission matches against
/// and a phase key in the plan JSON; an id carrying `/` from the directory path
/// would collide with path-like keys elsewhere.
fn loose_label(dir: &str, part: usize) -> String {
    let base = if dir.is_empty() {
        String::new()
    } else {
        slugify_sector(dir)
    };
    let suffix = if part == 1 {
        "root".to_string()
    } else {
        format!("root-{part}")
    };
    if base.is_empty() {
        suffix
    } else {
        format!("{base}-{suffix}")
    }
}

fn strip_prefix_dir<'a>(rel: &'a str, dir: &str) -> &'a str {
    if dir.is_empty() {
        rel
    } else {
        rel.strip_prefix(dir)
            .and_then(|r| r.strip_prefix('/'))
            .unwrap_or(rel)
    }
}

fn make_sector(
    dir: &str,
    files: &[&SourceFile],
    bytes: u64,
    lines: u32,
    opts: &SectorOptions,
) -> Sector {
    let id = if dir.is_empty() {
        "root".to_string()
    } else {
        slugify_sector(dir)
    };
    make_sector_named(&id, dir, files, bytes, lines, opts)
}

fn make_sector_named(
    id: &str,
    dir: &str,
    files: &[&SourceFile],
    bytes: u64,
    lines: u32,
    opts: &SectorOptions,
) -> Sector {
    let mut names: Vec<String> = files.iter().map(|f| f.rel.clone()).collect();
    names.sort();
    let units = build_units(files.iter().copied(), opts);
    // "Oversized" means unfixable by chunking, not merely big: a sector over the
    // byte ceiling whose units all fit is perfectly dispatchable, one unit at a
    // time. What an operator needs warned about is the case where a single unit
    // still does not fit a context window.
    let oversized = units.iter().any(|u| u.bytes > opts.max_unit_bytes);
    Sector {
        id: id.to_string(),
        path: dir.to_string(),
        files: names,
        bytes,
        lines,
        units,
        depends_on: Vec::new(),
        oversized,
    }
}

fn slugify_sector(dir: &str) -> String {
    let mut out = String::with_capacity(dir.len());
    for c in dir.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "root".to_string()
    } else {
        trimmed
    }
}

/// Split oversized files into line-range chunks so no single dispatch is handed
/// more source than a context window holds.
///
/// Chunks are filled by accumulating lines up to the byte ceiling, not by
/// dividing the line count evenly. The even division is wrong on real files:
/// `chunk_count = min(ceil(bytes / ceiling), total_lines)` collapses to 1 on a
/// file with very long lines, so the "chunks" come out just as oversized as the
/// input and the ceiling silently stops meaning anything.
fn build_units<'a>(
    files: impl Iterator<Item = &'a SourceFile>,
    opts: &SectorOptions,
) -> Vec<SectorUnit> {
    let mut units = Vec::new();
    for f in files {
        if f.bytes <= opts.max_unit_bytes {
            units.push(SectorUnit {
                rel: f.rel.clone(),
                bytes: f.bytes,
                line_start: 1,
                line_end: f.lines,
                chunk: 1,
                chunks: 1,
            });
            continue;
        }
        // Exact line boundaries need the file; without it, fall back to one
        // whole-file unit and let the dispatch report the problem itself rather
        // than inventing a split that does not line up with the source.
        let Ok(repo_bytes) = read_file_for_chunking(&f.rel, opts) else {
            units.push(SectorUnit {
                rel: f.rel.clone(),
                bytes: f.bytes,
                line_start: 1,
                line_end: f.lines,
                chunk: 1,
                chunks: 1,
            });
            continue;
        };
        let text = String::from_utf8_lossy(&repo_bytes);
        let line_starts = line_offsets(&text);
        let total_lines = line_count(&text).max(1);

        // First pass: where the chunks break. Second: number them, because a
        // unit has to report `chunk n of m` and `m` is not known until the end.
        let mut spans: Vec<(u32, u32, u64)> = Vec::new();
        let mut start = 1u32;
        while start <= total_lines {
            let mut end = start;
            let mut end_bytes = span_bytes(&text, &line_starts, start, end);
            while end < total_lines {
                let next = end + 1;
                let next_bytes = span_bytes(&text, &line_starts, start, next);
                // No "at least one line" guard is needed here: `end` starts at
                // `start`, so the chunk always owns the line it opened with even
                // when that one line is longer than the ceiling. Testing
                // `end > start` instead — the obvious way to write the guard —
                // silently lets an already-over-ceiling chunk absorb a second
                // line, which then looks like a chunker that cannot respect its
                // own limit.
                if next_bytes > opts.max_unit_bytes {
                    break;
                }
                end = next;
                end_bytes = next_bytes;
            }
            spans.push((start, end, end_bytes));
            start = end + 1;
        }
        if spans.is_empty() {
            spans.push((1, total_lines, f.bytes));
        }
        let count = spans.len() as u32;
        for (i, (from, to, bytes)) in spans.into_iter().enumerate() {
            units.push(SectorUnit {
                rel: f.rel.clone(),
                bytes,
                line_start: from,
                line_end: to,
                chunk: i as u32 + 1,
                chunks: count,
            });
        }
    }
    units
}

/// Chunking needs the file's bytes, and the caller only handed us a path
/// relative to the repo root. The plan is built from that root, so the unit
/// builder resolves against the same root it was given via a thread-local set by
/// [`plan_sectors_with_parallelism`]'s caller — kept simple here by re-reading
/// through the absolute path stored in the options-independent `CURRENT_ROOT`.
fn read_file_for_chunking(rel: &str, _opts: &SectorOptions) -> std::io::Result<Vec<u8>> {
    CURRENT_ROOT.with(|root| {
        let base = root.borrow();
        match base.as_ref() {
            Some(p) => std::fs::read(p.join(rel)),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no repository root registered for chunking",
            )),
        }
    })
}

thread_local! {
    /// Repo root for the in-progress decomposition, so unit chunking can read
    /// the oversized files it needs without threading a `&Path` through every
    /// helper. Set for the duration of one `plan_sectors` call.
    static CURRENT_ROOT: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Byte offset of the start of each 1-based line.
fn line_offsets(text: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

fn span_bytes(text: &str, starts: &[usize], line_start: u32, line_end: u32) -> u64 {
    let from = starts
        .get((line_start.saturating_sub(1)) as usize)
        .copied()
        .unwrap_or(0);
    let to = starts.get(line_end as usize).copied().unwrap_or(text.len());
    to.saturating_sub(from) as u64
}

/// Extract the module paths a file imports, as `/`-separated dotted module
/// names resolved against `source_root`.
///
/// Deliberately a line-oriented scanner, not a parser: it only has to be good
/// enough to order sectors, and a real Python/TS parser would be a dependency
/// for a heuristic. False positives cost a spurious edge; false negatives cost
/// a wave that could have been tighter. Neither breaks the plan.
fn imports_of(language: &str, text: &str, own_rel: &str, source_root: &str) -> Vec<String> {
    let mut out = Vec::new();
    let own_dir = own_rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        match language {
            "python" => {
                if let Some(rest) = line.strip_prefix("from ") {
                    if let Some((module, _)) = rest.split_once(" import ") {
                        out.extend(resolve_python(module.trim(), own_dir, source_root));
                    }
                } else if let Some(rest) = line.strip_prefix("import ") {
                    for piece in rest.split(',') {
                        let module = piece
                            .trim()
                            .split_once(" as ")
                            .map(|(m, _)| m.trim())
                            .unwrap_or_else(|| piece.trim());
                        if !module.is_empty() {
                            out.push(normalise_module(module, source_root));
                        }
                    }
                }
            }
            "typescript" | "javascript" => {
                let Some(from_idx) = line.find(" from ") else {
                    continue;
                };
                let tail = &line[from_idx + 6..];
                // `\u{22}` rather than a literal `"` inside a char literal:
                // the plain spelling is unambiguous to rustc but not to every
                // tool that rewrites this file, and a single mangled byte here
                // mis-lexes the rest of the module.
                let Some(spec) = tail.split(['\u{22}', '`']).nth(1) else {
                    continue;
                };
                if !spec.starts_with('.') {
                    continue; // bare package import: not our source
                }
                let joined = join_relative(own_dir, spec);
                if !joined.is_empty() {
                    out.push(strip_source_root(&joined, source_root));
                }
            }
            "rust" => {
                let Some(kw) = line.find("use ") else {
                    continue;
                };
                let tail = &line[kw + 4..];
                let module = tail
                    .split("::")
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('{')
                    .trim();
                if module.is_empty() || module == "crate" || module == "self" || module == "super" {
                    continue;
                }
                out.push(module.to_string());
            }
            _ => {}
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Resolve a Python `from X import ...` module spec, including relative dots.
fn resolve_python(module: &str, own_dir: &str, source_root: &str) -> Option<String> {
    if module.starts_with('.') {
        let dots = module.chars().take_while(|&c| c == '.').count();
        let tail = module[dots..].trim();
        // One dot = current package, so strip (dots - 1) trailing components.
        let mut parts: Vec<&str> = own_dir.split('/').filter(|p| !p.is_empty()).collect();
        for _ in 1..dots {
            parts.pop();
        }
        if tail.is_empty() {
            // `from . import x` names a sibling inside the current package; the
            // directory itself is the dependency.
            return Some(strip_source_root(&parts.join("/"), source_root));
        }
        let mut base = parts.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        base.extend(tail.split('.').map(|s| s.to_string()));
        return Some(strip_source_root(&base.join("/"), source_root));
    }
    Some(normalise_module(module, source_root))
}

fn normalise_module(module: &str, source_root: &str) -> String {
    strip_source_root(&module.replace('.', "/"), source_root)
}

/// Drop a leading `litellm/` so import keys and file keys are in the same space.
fn strip_source_root(path: &str, source_root: &str) -> String {
    if source_root.is_empty() {
        return path.to_string();
    }
    let prefix = format!("{source_root}/");
    path.strip_prefix(&prefix).unwrap_or(path).to_string()
}

/// Join `./x` or `../y` against the importing file's directory.
fn join_relative(own_dir: &str, spec: &str) -> String {
    let mut parts: Vec<&str> = own_dir.split('/').filter(|p| !p.is_empty()).collect();
    let mut rest = spec;
    while rest.starts_with('.') {
        rest = &rest[1..];
        if rest.starts_with('/') {
            rest = &rest[1..];
            continue;
        }
        parts.pop();
    }
    let mut base: Vec<String> = parts.iter().map(|s| s.to_string()).collect();
    if !rest.is_empty() {
        base.extend(rest.split('/').map(|s| s.to_string()));
    }
    base.join("/")
}

/// Fill in `depends_on` for every sector from the import graph.
fn link_dependencies(source_root: &str, sectors: &mut [Sector]) {
    // file (source_root-relative, no extension) -> sector id
    let mut owner: HashMap<String, String> = HashMap::new();
    for s in sectors.iter() {
        for f in &s.files {
            for key in module_keys(f, source_root) {
                owner.entry(key).or_insert_with(|| s.id.clone());
            }
        }
    }

    let root = CURRENT_ROOT.with(|r| r.borrow().clone());
    let Some(base) = root else { return };

    for s in sectors.iter_mut() {
        let mut deps: BTreeSet<String> = BTreeSet::new();
        for f in &s.files {
            let Ok(bytes) = std::fs::read(base.join(f)) else {
                continue;
            };
            // Cap the scan: a 700 KB file's imports are all in its first
            // stretches anyway (module headers), and reading every byte of every
            // file to find `import` lines is the expensive part of planning.
            let text = String::from_utf8_lossy(&bytes[..bytes.len().min(400_000)]);
            let language = language_of(f);
            for imported in imports_of(language, &text, f, source_root) {
                if let Some(dep) = resolve_owner(&imported, &owner) {
                    if dep != s.id {
                        deps.insert(dep);
                    }
                }
            }
        }
        s.depends_on = deps.into_iter().collect();
    }
}

/// The module keys a file answers to.
///
/// Only exact module paths — not every ancestor package. Registering ancestors
/// is the obvious way to make `from a.b import c` resolve, and it is wrong: it
/// turns every import in the repository into an edge to whichever sector
/// happened to claim `a/b` first. On LiteLLM that manufactured thousands of
/// spurious cycles and a wave schedule that encoded nothing. A package's
/// `__init__.py` is already registered as the package itself, which is the
/// sector that genuinely owns the name.
fn module_keys(rel: &str, source_root: &str) -> Vec<String> {
    let stripped = strip_source_root(rel, source_root);
    let no_ext = stripped
        .rsplit_once('.')
        .map(|(a, _)| a.to_string())
        .unwrap_or_else(|| stripped.clone());
    let base = no_ext
        .strip_suffix("/__init__")
        .unwrap_or(&no_ext)
        .to_string();
    let mut keys = vec![base.clone()];
    // And the un-stripped form, for imports written with the package prefix.
    if !source_root.is_empty() {
        keys.push(format!("{source_root}/{base}"));
    }
    keys
}

/// Resolve an imported module against the owner index.
///
/// Exact match, then one component trimmed. `from a.b import symbol` and
/// `import a.b.symbol` are indistinguishable without a real parser, so the trim
/// covers the symbol case. It stops at one: walking every ancestor is what
/// manufactured the spurious edges [`module_keys`] refuses to register.
fn resolve_owner(imported: &str, owner: &HashMap<String, String>) -> Option<String> {
    let path = imported.replace('.', "/");
    if let Some(id) = owner.get(&path) {
        return Some(id.clone());
    }
    let trimmed = path.rsplit_once('/').map(|(head, _)| head)?;
    owner.get(trimmed).cloned()
}

/// Group sectors into dependency waves (longest-path depth), breaking cycles
/// deterministically. Returns the waves and how many edges were dropped.
fn assign_waves(sectors: &mut [Sector]) -> (Vec<Vec<String>>, usize) {
    let ids: Vec<String> = sectors.iter().map(|s| s.id.clone()).collect();
    let id_set: BTreeSet<&str> = ids.iter().map(|s| s.as_str()).collect();

    // Keep only edges to sectors that exist in this plan.
    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for s in sectors.iter() {
        let deps: BTreeSet<String> = s
            .depends_on
            .iter()
            .filter(|d| id_set.contains(d.as_str()) && *d != &s.id)
            .cloned()
            .collect();
        edges.insert(s.id.clone(), deps);
    }

    let broken = remove_back_edges(&mut edges);

    // Rewrite each sector's deps to the acyclic set actually used for ordering.
    for s in sectors.iter_mut() {
        if let Some(deps) = edges.get(&s.id) {
            s.depends_on = deps.iter().cloned().collect();
        }
    }

    let order = topo_order(&edges).unwrap_or_else(|| ids.clone());
    let mut depth: HashMap<String, usize> = HashMap::new();
    for id in &order {
        let d = edges
            .get(id)
            .map(|deps| {
                deps.iter()
                    .map(|dep| depth.get(dep).copied().unwrap_or(0) + 1)
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        depth.insert(id.clone(), d);
    }
    let max_depth = depth.values().copied().max().unwrap_or(0);
    let mut waves: Vec<Vec<String>> = vec![Vec::new(); max_depth + 1];
    for (id, d) in &depth {
        waves[*d].push(id.clone());
    }
    for w in waves.iter_mut() {
        w.sort();
    }
    waves.retain(|w| !w.is_empty());
    (waves, broken)
}

/// Kahn's algorithm over `node -> {nodes it waits on}`.
///
/// `ready` is kept sorted and always drained from the front, so two runs over
/// the same graph emit the same order. That matters more than it sounds: the
/// order decides which sector a broken cycle sacrifices, so a non-deterministic
/// sort would make the plan change between runs and break resume.
fn topo_order(edges: &BTreeMap<String, BTreeSet<String>>) -> Option<Vec<String>> {
    let mut waiting: BTreeMap<&str, usize> = BTreeMap::new();
    for node in edges.keys() {
        waiting.entry(node.as_str()).or_insert(0);
    }
    let mut reverse: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (node, deps) in edges {
        waiting.insert(node.as_str(), deps.len());
        for d in deps {
            reverse.entry(d.as_str()).or_default().push(node.as_str());
        }
    }
    let mut ready: Vec<&str> = waiting
        .iter()
        .filter(|(_, n)| **n == 0)
        .map(|(k, _)| *k)
        .collect();
    ready.sort_unstable();
    let mut order = Vec::with_capacity(edges.len());
    while let Some(node) = ready.first().copied() {
        ready.remove(0);
        order.push(node.to_string());
        let mut newly: Vec<&str> = Vec::new();
        if let Some(dependents) = reverse.get(node) {
            for dep in dependents {
                if let Some(n) = waiting.get_mut(dep) {
                    *n = n.saturating_sub(1);
                    if *n == 0 {
                        newly.push(dep);
                    }
                }
            }
        }
        newly.sort_unstable();
        for n in newly {
            if !ready.contains(&n) {
                ready.push(n);
                ready.sort_unstable();
            }
        }
    }
    if order.len() == edges.len() {
        Some(order)
    } else {
        None
    }
}

/// Drop every back edge found by one DFS, in sorted node and sorted neighbour
/// order. Returns how many were dropped.
///
/// The obvious implementation — find a cycle, remove one edge, restart the
/// topological sort — is O(E) restarts of an O(V+E) sort. On LiteLLM, whose
/// `__init__.py` re-exports make the sector graph heavily cyclic, that measured
/// in minutes. One pass that removes every back edge it meets is linear and
/// equally deterministic, because the walk order is fixed.
///
/// Iterative rather than recursive: a chain of a few thousand sectors is not
/// hypothetical at this scale, and a stack overflow inside a planning step
/// would take the whole process down.
fn remove_back_edges(edges: &mut BTreeMap<String, BTreeSet<String>>) -> usize {
    // Snapshot the adjacency first: the walk removes edges from `edges` while it
    // is iterating, and mutating a map mid-walk is how you skip neighbours.
    let adjacency: BTreeMap<String, Vec<String>> = edges
        .iter()
        .map(|(k, v)| (k.clone(), v.iter().cloned().collect()))
        .collect();
    let roots: Vec<String> = adjacency.keys().cloned().collect();

    let mut state: HashMap<String, u8> = HashMap::new(); // 0 unseen, 1 on stack, 2 done
    let mut removed = 0usize;

    for root in &roots {
        if state.get(root.as_str()).copied().unwrap_or(0) != 0 {
            continue;
        }
        state.insert(root.clone(), 1);
        let mut stack: Vec<(String, usize)> = vec![(root.clone(), 0)];
        while let Some((node, idx)) = stack.last().cloned() {
            let neighbours = adjacency.get(&node).cloned().unwrap_or_default();
            if idx >= neighbours.len() {
                state.insert(node, 2);
                stack.pop();
                continue;
            }
            if let Some(top) = stack.last_mut() {
                top.1 += 1;
            }
            let next = &neighbours[idx];
            match state.get(next.as_str()).copied().unwrap_or(0) {
                // On the stack already: this edge closes a cycle.
                1 => {
                    if let Some(deps) = edges.get_mut(&node) {
                        if deps.remove(next) {
                            removed += 1;
                        }
                    }
                }
                0 => {
                    state.insert(next.clone(), 1);
                    stack.push((next.clone(), 0));
                }
                _ => {}
            }
        }
    }
    removed
}

fn summarise(
    plan: &mut SectorPlan,
    cycles_broken: usize,
    skipped: SkippedGenerated,
    root: &SourceRootDecision,
    opts: &SectorOptions,
    parallelism: usize,
) {
    let mut t = SectorTotals::default();
    t.sectors = plan.sectors.len();
    t.waves = plan.waves.len();
    t.largest_wave = plan.waves.iter().map(|w| w.len()).max().unwrap_or(0);
    t.cycles_broken = cycles_broken;
    t.parallelism = parallelism;
    t.generated_skipped = skipped.files;
    t.generated_bytes = skipped.bytes;
    t.generated_examples = skipped.examples.clone();
    // Carried through verbatim: these are the numbers that say whether the
    // sector totals below describe the repository or a slice of it.
    t.outside_root_files = root.outside_files;
    t.outside_root_bytes = root.outside_bytes;
    t.outside_root_examples = root.outside_examples.clone();
    t.root_declined = root.declined.clone();
    for s in &plan.sectors {
        t.files += s.files.len();
        t.bytes += s.bytes;
        t.lines = t.lines.saturating_add(s.lines);
        t.dispatches += s.units.len();
        if s.bytes > opts.max_sector_bytes {
            t.sectors_over_budget += 1;
        }
        for u in &s.units {
            if u.is_partial() {
                t.chunks += 1;
            }
            if u.bytes > opts.max_unit_bytes {
                // Only reachable when one line is longer than the ceiling, so
                // the unit cannot be split any finer. Worth counting separately:
                // unlike `sectors_over_budget` this one means a dispatch will be
                // handed more source than it can hold.
                t.units_over_ceiling += 1;
            }
        }
    }
    t.files_chunked = plan
        .sectors
        .iter()
        .flat_map(|s| s.units.iter())
        .filter(|u| u.is_partial())
        .map(|u| u.rel.clone())
        .collect::<BTreeSet<String>>()
        .len();
    t.est_model_secs = (t.dispatches as u64).saturating_mul(opts.per_dispatch_secs);
    t.est_wall_hours_at_parallelism =
        (t.est_model_secs as f64) / (parallelism.max(1) as f64) / 3600.0;
    plan.totals = t;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Write a file of exactly `bytes` bytes with a newline every 40th byte.
    ///
    /// Exact size matters: the chunking and effort-estimate tests assert on
    /// `ceil(bytes / max_unit_bytes)`, so a helper that padded the file would
    /// silently change the expected chunk count.
    fn write(root: &Path, rel: &str, bytes: usize) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let body: String = (0..bytes)
            .map(|i| if i % 40 == 39 { '\n' } else { 'a' })
            .collect();
        debug_assert_eq!(body.len(), bytes);
        fs::write(path, body).unwrap();
    }

    /// Force one directory to become one sector regardless of how small it is,
    /// so a test can assert on per-package edges instead of getting a single
    /// sector for the whole tree.
    fn one_sector_per_dir() -> SectorOptions {
        SectorOptions {
            max_files_per_sector: 1,
            ..Default::default()
        }
    }

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn a_big_package_is_split_into_sectors_under_the_ceiling() {
        let d = tmp();
        let root = d.path();
        for sub in ["alpha", "beta", "gamma"] {
            for i in 0..6 {
                write(root, &format!("pkg/{sub}/mod{i}.py"), 1_500);
            }
        }
        let opts = SectorOptions {
            max_sector_bytes: 10_000,
            max_files_per_sector: 40,
            ..Default::default()
        };
        let plan = plan_sectors_scoped(root, &opts, 1);
        assert_eq!(plan.source_root, "pkg", "source root detection");
        assert_eq!(plan.sectors.len(), 3, "one sector per subpackage");
        for s in &plan.sectors {
            assert!(
                s.bytes <= opts.max_sector_bytes,
                "sector {} is {} bytes",
                s.id,
                s.bytes
            );
            assert_eq!(s.files.len(), 6);
        }
    }

    #[test]
    fn a_subpackage_over_the_ceiling_is_split_again_rather_than_emitted_whole() {
        let d = tmp();
        let root = d.path();
        for i in 0..6 {
            write(root, &format!("pkg/alpha/mod{i}.py"), 3_000);
        }
        let opts = SectorOptions {
            max_sector_bytes: 10_000,
            max_files_per_sector: 40,
            ..Default::default()
        };
        let plan = plan_sectors_scoped(root, &opts, 1);
        assert_eq!(
            plan.sectors.len(),
            2,
            "18 KB over a 10 KB ceiling must bin-pack into two sectors, got {:?}",
            plan.sectors.iter().map(|s| &s.id).collect::<Vec<_>>()
        );
        for s in &plan.sectors {
            assert!(s.bytes <= opts.max_sector_bytes, "{} = {}", s.id, s.bytes);
            assert!(s.id.starts_with("pkg-alpha-root"), "{}", s.id);
        }
    }

    #[test]
    fn huge_loose_files_are_bin_packed_not_left_as_one_sector() {
        let d = tmp();
        let root = d.path();
        for name in ["router.py", "utils.py", "main.py", "cost.py"] {
            write(root, &format!("pkg/{name}"), 20_000);
        }
        let opts = SectorOptions {
            max_sector_bytes: 25_000,
            max_files_per_sector: 40,
            ..Default::default()
        };
        let plan = plan_sectors_scoped(root, &opts, 1);
        assert!(
            plan.sectors.len() >= 3,
            "80 KB of loose files must not be one sector, got {}",
            plan.sectors.len()
        );
        for s in &plan.sectors {
            assert!(
                s.bytes <= opts.max_sector_bytes,
                "{} = {} bytes",
                s.id,
                s.bytes
            );
        }
    }

    #[test]
    fn an_oversized_file_is_chunked_into_line_ranges() {
        let d = tmp();
        let root = d.path();
        write(root, "pkg/router.py", 60_000);
        let opts = SectorOptions {
            max_sector_bytes: 200_000,
            max_unit_bytes: 20_000,
            ..Default::default()
        };
        let plan = plan_sectors_scoped(root, &opts, 1);
        let sector = &plan.sectors[0];
        assert_eq!(sector.files, vec!["pkg/router.py".to_string()]);
        assert_eq!(sector.units.len(), 3, "60 KB / 20 KB = 3 chunks");
        assert_eq!(sector.units[0].chunk, 1);
        assert_eq!(sector.units[2].chunk, 3);
        assert_eq!(sector.units[2].chunks, 3);
        assert_eq!(sector.units[0].line_start, 1);
        assert!(
            sector.units[0].line_end < sector.units[1].line_start,
            "chunks must not overlap"
        );
        assert_eq!(
            sector.units[0].line_end + 1,
            sector.units[1].line_start,
            "chunks must be contiguous"
        );
        assert!(sector.units.iter().all(|u| u.is_partial()));
        assert_eq!(plan.totals.files_chunked, 1);
        assert_eq!(plan.totals.chunks, 3);
        assert_eq!(plan.totals.dispatches, 3);
    }

    #[test]
    fn imports_between_subpackages_become_sector_edges_and_waves() {
        let d = tmp();
        let root = d.path();
        fs::create_dir_all(root.join("pkg/types")).unwrap();
        fs::create_dir_all(root.join("pkg/llms")).unwrap();
        fs::create_dir_all(root.join("pkg/proxy")).unwrap();
        fs::write(root.join("pkg/types/base.py"), "class Model: pass\n").unwrap();
        // client imports types; api imports client -> depth 0,1,2
        fs::write(
            root.join("pkg/llms/client.py"),
            "from pkg.types.base import Model\n",
        )
        .unwrap();
        fs::write(
            root.join("pkg/proxy/api.py"),
            "from pkg.llms.client import call\n",
        )
        .unwrap();

        let plan = plan_sectors_scoped(root, &one_sector_per_dir(), 1);
        let ids: Vec<&str> = plan.sectors.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"pkg-types"), "{ids:?}");
        assert!(ids.contains(&"pkg-llms"), "{ids:?}");
        assert!(ids.contains(&"pkg-proxy"), "{ids:?}");
        let types = plan.sector("pkg-types").unwrap();
        let llms = plan.sector("pkg-llms").unwrap();
        let proxy = plan.sector("pkg-proxy").unwrap();
        assert!(types.depends_on.is_empty(), "{:?}", types.depends_on);
        assert_eq!(llms.depends_on, vec!["pkg-types".to_string()]);
        assert_eq!(proxy.depends_on, vec!["pkg-llms".to_string()]);
        assert_eq!(plan.waves.len(), 3, "{:?}", plan.waves);
        assert_eq!(plan.waves[0], vec!["pkg-types".to_string()]);
        assert_eq!(plan.waves[2], vec!["pkg-proxy".to_string()]);
        assert_eq!(plan.totals.cycles_broken, 0);
    }

    #[test]
    fn relative_imports_resolve_within_their_own_package() {
        let d = tmp();
        let root = d.path();
        fs::create_dir_all(root.join("pkg/a")).unwrap();
        fs::create_dir_all(root.join("pkg/b")).unwrap();
        fs::write(root.join("pkg/a/one.py"), "value = 1\n").unwrap();
        fs::write(root.join("pkg/b/two.py"), "from ..a.one import value\n").unwrap();
        let plan = plan_sectors_scoped(root, &one_sector_per_dir(), 1);
        let b = plan.sector("pkg-b").unwrap();
        assert_eq!(
            b.depends_on,
            vec!["pkg-a".to_string()],
            "`from ..a.one` must resolve to pkg/a"
        );
    }

    #[test]
    fn a_dependency_cycle_is_broken_and_counted_not_fatal() {
        let d = tmp();
        let root = d.path();
        fs::create_dir_all(root.join("pkg/x")).unwrap();
        fs::create_dir_all(root.join("pkg/y")).unwrap();
        fs::write(root.join("pkg/x/a.py"), "from pkg.y.b import thing\n").unwrap();
        fs::write(root.join("pkg/y/b.py"), "from pkg.x.a import other\n").unwrap();
        let plan = plan_sectors_scoped(root, &one_sector_per_dir(), 1);
        assert_eq!(plan.totals.cycles_broken, 1);
        assert!(!plan.waves.is_empty(), "waves must still be produced");
        let total_in_waves: usize = plan.waves.iter().map(|w| w.len()).sum();
        assert_eq!(total_in_waves, 2, "every sector must land in a wave");
    }

    #[test]
    fn test_files_are_excluded_unless_asked_for() {
        let d = tmp();
        let root = d.path();
        write(root, "pkg/core.py", 200);
        write(root, "pkg/tests/test_core.py", 200);
        write(root, "tests/test_pkg.py", 200);
        let plan = plan_sectors_scoped(root, &SectorOptions::default(), 1);
        let all: Vec<&str> = plan
            .sectors
            .iter()
            .flat_map(|s| s.files.iter().map(|f| f.as_str()))
            .collect();
        assert_eq!(all, vec!["pkg/core.py"], "{all:?}");
        let with_tests = SectorOptions {
            include_tests: true,
            ..Default::default()
        };
        let plan = plan_sectors_scoped(root, &with_tests, 1);
        // Three, not two. This used to assert two on the reasoning that `tests/`
        // sits outside the detected package root and the plan is scoped to it —
        // but that is the same silent drop the coverage gate now refuses, and
        // here it was worse: the caller had explicitly asked for the test suite
        // and was handed a plan missing the repo's own integration tests. Two
        // 200-byte files against one leaves `pkg/` at 2/3 coverage, under the
        // floor, so nothing is scoped away.
        assert_eq!(plan.totals.files, 3, "{:?}", plan.totals);
        assert_eq!(plan.source_root, "", "a partial root must not be adopted");
        assert!(
            !plan.totals.root_declined.is_empty(),
            "the refusal to scope has to be reported, not just correct"
        );
    }

    #[test]
    fn non_source_directories_are_not_walked() {
        let d = tmp();
        let root = d.path();
        write(root, "pkg/core.py", 200);
        write(root, "docs/guide.py", 200);
        write(root, ".github/hook.py", 200);
        write(root, "node_modules/dep/index.js", 200);
        let plan = plan_sectors_scoped(root, &SectorOptions::default(), 1);
        assert_eq!(plan.totals.files, 1, "{:?}", plan.totals);
    }

    fn sf(rel: &str, bytes: u64) -> SourceFile {
        SourceFile {
            rel: rel.to_string(),
            bytes,
            lines: (bytes / 40).max(1) as u32,
            language: "python".to_string(),
        }
    }

    /// The pypa/packaging regression.
    ///
    /// [`NON_SOURCE_DIRS`] is matched as a bare component name at *any* depth,
    /// so an entry that is also a real library's own package name removes that
    /// library from the plan before root detection ever sees it. `packaging` was
    /// such an entry: the collector discarded all 480 KB of `src/packaging/`,
    /// and the plan then nominated `tasks/` — 16 KB of nox helpers — as the
    /// package root of a library it had never collected, and went on to report
    /// the rewrite complete. Nothing in the output said anything was missing.
    #[test]
    fn a_library_directory_named_like_a_junk_directory_is_still_scheduled() {
        let d = tmp();
        let root = d.path();
        // Shape mirrors the real repository: the library under `src/`, its own
        // package directory carrying a name that used to be on the exclusion
        // list, and maintainer tooling beside it.
        for i in 0..6 {
            write(root, &format!("src/packaging/m{i}.py"), 20_000);
        }
        write(root, "tasks/nox.py", 4_000);
        let plan = plan_sectors_scoped(root, &SectorOptions::default(), 1);

        let scheduled: Vec<&str> = plan
            .sectors
            .iter()
            .flat_map(|s| s.files.iter().map(|f| f.as_str()))
            .collect();
        assert_eq!(
            scheduled
                .iter()
                .filter(|f| f.starts_with("src/packaging/"))
                .count(),
            6,
            "the library itself must be in the plan: {scheduled:?}"
        );
        assert_eq!(plan.source_root, "src", "{:?}", plan.totals);
        // 120 KB of library against 4 KB of tooling, so scoping to `src/` costs
        // almost nothing and is adopted rather than declined.
        assert!(
            plan.totals.root_coverage() > 0.95,
            "coverage was {:.3}",
            plan.totals.root_coverage()
        );
        assert_eq!(plan.totals.outside_root_files, 1, "{:?}", plan.totals);
        assert_eq!(plan.totals.outside_root_bytes, 4_000);
        assert!(
            plan.totals.root_declined.is_empty(),
            "a root that was adopted declined nothing"
        );
        // The dropped tooling must still be visible: adopting the root was
        // correct, but "correct" is not the same as "unreported".
        let f = plan.totals.feasibility();
        assert!(f.contains("tasks/nox.py"), "{f}");
    }

    /// Two peer packages and no dominant root. The old gate asked only whether
    /// the largest directory cleared a quarter of the tree, so it would have
    /// adopted `backend/` here and dropped `frontend/` without a word. The
    /// coverage gate refuses instead, because byte counts cannot distinguish a
    /// monorepo of separate sub-projects from one library split in two.
    #[test]
    fn a_root_that_would_drop_most_of_the_tree_is_declined_not_adopted() {
        let d = tmp();
        let root = d.path();
        for i in 0..5 {
            write(root, &format!("backend/m{i}.py"), 10_000);
        }
        for i in 0..4 {
            write(root, &format!("frontend/m{i}.ts"), 10_000);
        }
        let plan = plan_sectors_scoped(root, &SectorOptions::default(), 1);
        assert_eq!(plan.source_root, "", "no root may be adopted");
        assert_eq!(plan.totals.files, 9, "everything collected is scheduled");
        assert_eq!(plan.totals.outside_root_files, 0);
        assert_eq!(plan.totals.outside_root_bytes, 0);
        assert_eq!(plan.totals.root_coverage(), 1.0);
        // The refusal has to name the directory and the arithmetic, or an
        // operator cannot tell it apart from a repo that simply has no package.
        let declined = &plan.totals.root_declined;
        assert!(declined.contains("backend/"), "{declined}");
        assert!(declined.contains('%'), "{declined}");
        assert!(declined.contains("source_root"), "{declined}");
        assert!(
            plan.totals.feasibility().contains("SCOPE:"),
            "{}",
            plan.totals.feasibility()
        );
    }

    /// The deliberate escape hatch. An operator who knows the other directories
    /// are separate sub-projects can scope to one, and the cost is then measured
    /// and reported rather than inferred from a heuristic that had to guess.
    #[test]
    fn an_explicit_source_root_is_honoured_and_its_cost_reported() {
        let d = tmp();
        let root = d.path();
        for i in 0..5 {
            write(root, &format!("backend/m{i}.py"), 10_000);
        }
        for i in 0..4 {
            write(root, &format!("frontend/m{i}.ts"), 10_000);
        }
        let opts = SectorOptions {
            source_root: "backend".into(),
            ..Default::default()
        };
        let plan = plan_sectors_scoped(root, &opts, 1);
        assert_eq!(plan.source_root, "backend");
        assert_eq!(plan.totals.files, 5, "{:?}", plan.totals);
        assert_eq!(plan.totals.outside_root_files, 4);
        assert_eq!(plan.totals.outside_root_bytes, 40_000);
        assert!(
            plan.totals.root_declined.is_empty(),
            "nothing was declined — the operator chose this scope"
        );
        let cov = plan.totals.root_coverage();
        assert!((cov - 5.0 / 9.0).abs() < 1e-9, "{cov}");
        // And the scope must reach the one sentence an operator actually reads,
        // naming a file that will not be ported.
        let f = plan.totals.feasibility();
        assert!(f.contains("SCOPE WARNING"), "{f}");
        assert!(f.contains("frontend/m0.ts"), "{f}");
    }

    /// A typo in `source_root` must not produce an empty plan that reports
    /// success. That is the original defect arriving by a different route.
    #[test]
    fn an_explicit_source_root_that_matches_nothing_says_so() {
        let d = tmp();
        let root = d.path();
        write(root, "backend/m0.py", 10_000);
        let opts = SectorOptions {
            source_root: "src/nope".into(),
            ..Default::default()
        };
        let plan = plan_sectors_scoped(root, &opts, 1);
        assert!(plan.sectors.is_empty());
        assert_eq!(plan.totals.dispatches, 0);
        let declined = &plan.totals.root_declined;
        assert!(declined.contains("matched 0 of 1"), "{declined}");
        assert!(declined.contains("src/nope"), "{declined}");
        // Not widened to compensate: the operator asked for that scope, and
        // quietly porting something else would be worse than porting nothing.
        let f = plan.totals.feasibility();
        assert!(f.contains("ports nothing"), "{f}");
        assert!(f.contains("SCOPE WARNING"), "{f}");
    }

    #[test]
    fn decide_source_root_accounts_for_every_file_it_drops() {
        let files = vec![
            sf("pkg/a.py", 900),
            sf("pkg/b.py", 50),
            sf("tools/c.py", 50),
        ];
        let d = decide_source_root(&files);
        assert_eq!(d.root, "pkg");
        assert_eq!(d.inside_files, 2);
        assert_eq!(d.inside_bytes, 950);
        assert_eq!(d.outside_files, 1);
        assert_eq!(d.outside_bytes, 50);
        assert_eq!(d.outside_examples, vec!["tools/c.py (0 KB)".to_string()]);
        // The identity the whole accounting rests on: scheduled plus dropped
        // must equal collected, so a byte cannot leave the plan uncounted.
        assert_eq!(d.inside_bytes + d.outside_bytes, 1_000);
        assert!((d.coverage() - 0.95).abs() < 1e-9, "{}", d.coverage());
    }

    /// Examples are capped so a large monorepo cannot put two thousand paths in
    /// a report, but the *counts* are not capped — the cap is on prose, not on
    /// arithmetic.
    #[test]
    fn the_dropped_file_examples_are_capped_but_the_counts_are_not() {
        let mut files = vec![sf("pkg/keep.py", 1_000_000)];
        for i in 0..50 {
            files.push(sf(&format!("tools/t{i}.py"), 100));
        }
        let d = decide_source_root(&files);
        assert_eq!(d.root, "pkg");
        assert_eq!(d.outside_files, 50);
        assert_eq!(d.outside_bytes, 5_000);
        assert_eq!(d.outside_examples.len(), 5);
    }

    #[test]
    fn an_empty_tree_has_no_root_and_total_coverage() {
        let d = decide_source_root(&[]);
        assert_eq!(d.root, "");
        assert_eq!(d.inside_files, 0);
        assert_eq!(d.coverage(), 1.0, "nothing collected, nothing dropped");
        assert!(d.declined.is_empty());
        // Same via the totals, whose `Default` must not read as "0% scheduled".
        assert_eq!(SectorTotals::default().root_coverage(), 1.0);
    }

    #[test]
    fn a_flat_repo_still_decomposes() {
        let d = tmp();
        let root = d.path();
        for i in 0..4 {
            write(root, &format!("mod{i}.py"), 1_000);
        }
        let plan = plan_sectors_scoped(root, &SectorOptions::default(), 1);
        assert_eq!(plan.source_root, "", "no dominant package");
        assert_eq!(plan.totals.files, 4);
        assert!(!plan.sectors.is_empty());
    }

    #[test]
    fn planning_the_same_tree_twice_gives_the_same_plan() {
        let d = tmp();
        let root = d.path();
        for sub in ["a", "b", "c"] {
            for i in 0..4 {
                write(root, &format!("pkg/{sub}/m{i}.py"), 4_000);
            }
        }
        fs::write(root.join("pkg/b/m0.py"), "from pkg.a.m1 import x\n").unwrap();
        let opts = SectorOptions {
            max_sector_bytes: 20_000,
            ..Default::default()
        };
        let one = plan_sectors_scoped(root, &opts, 1);
        let two = plan_sectors_scoped(root, &opts, 1);
        assert_eq!(one, two, "decomposition must be reproducible");
    }

    #[test]
    fn the_effort_estimate_scales_with_dispatches_and_parallelism() {
        let d = tmp();
        let root = d.path();
        write(root, "pkg/big.py", 90_000);
        let opts = SectorOptions {
            max_sector_bytes: 200_000,
            max_unit_bytes: 30_000,
            per_dispatch_secs: 300,
            ..Default::default()
        };
        let serial = plan_sectors_scoped(root, &opts, 1);
        assert_eq!(serial.totals.dispatches, 3);
        assert_eq!(serial.totals.est_model_secs, 900);
        assert!((serial.totals.est_wall_hours_at_parallelism - 0.25).abs() < 1e-9);
        let parallel = plan_sectors_scoped(root, &opts, 4);
        assert_eq!(
            parallel.totals.est_model_secs, 900,
            "model time does not shrink with parallelism"
        );
        assert!((parallel.totals.est_wall_hours_at_parallelism - 0.0625).abs() < 1e-9);
    }

    #[test]
    fn sector_ids_are_unique_and_slugged() {
        let d = tmp();
        let root = d.path();
        for sub in ["llms/openai", "llms/anthropic", "proxy/auth"] {
            write(root, &format!("litellm/{sub}/mod.py"), 500);
        }
        let plan = plan_sectors_scoped(root, &one_sector_per_dir(), 1);
        let mut ids: Vec<&str> = plan.sectors.iter().map(|s| s.id.as_str()).collect();
        ids.sort_unstable();
        let mut uniq = ids.clone();
        uniq.dedup();
        assert_eq!(ids, uniq, "duplicate sector ids: {ids:?}");
        assert!(ids.contains(&"litellm-llms-openai"), "{ids:?}");
        assert!(ids.iter().all(|i| !i.contains('/')), "{ids:?}");
    }

    #[test]
    fn the_json_round_trips_the_totals_and_waves() {
        let d = tmp();
        let root = d.path();
        fs::create_dir_all(root.join("pkg/a")).unwrap();
        fs::create_dir_all(root.join("pkg/b")).unwrap();
        fs::write(root.join("pkg/a/m.py"), "value = 1\n").unwrap();
        fs::write(root.join("pkg/b/m.py"), "from pkg.a.m import value\n").unwrap();
        let plan = plan_sectors_scoped(root, &one_sector_per_dir(), 2);
        let v = plan.to_json();
        assert_eq!(v["source_root"], "pkg");
        assert_eq!(v["totals"]["sectors"], 2);
        assert_eq!(v["totals"]["parallelism"], 2);
        assert_eq!(v["waves"].as_array().unwrap().len(), 2);
        assert_eq!(v["sectors"].as_array().unwrap().len(), 2);
        assert!(v["sectors"][0]["units"].is_array());
        // The scope accounting has to survive the round trip, because the
        // mission runner reads the persisted plan rather than re-deriving it.
        let back = SectorPlan::from_json(&v).unwrap();
        assert_eq!(back, plan);
        assert_eq!(back.totals.root_coverage(), plan.totals.root_coverage());
    }

    /// A plan written before the scope fields existed must still load. Refusing
    /// to resume over a reporting field would strand hours of paid-for work, and
    /// the lenient read is the reason those five fields are not `?`-required
    /// alongside the rest.
    #[test]
    fn a_plan_from_an_older_build_still_loads_without_the_scope_fields() {
        let mut v = serde_json::json!({
            "repo_name": "old",
            "source_root": "pkg",
            "totals": {
                "files": 2, "bytes": 100, "lines": 10, "sectors": 1,
                "dispatches": 2, "waves": 1, "largest_wave": 1,
                "sectors_over_budget": 0, "files_chunked": 0, "chunks": 0,
                "cycles_broken": 0, "units_over_ceiling": 0,
                "generated_skipped": 0, "generated_bytes": 0,
                "generated_examples": [],
                "est_model_secs": 600,
                "est_wall_hours_at_parallelism": 0.1,
                "parallelism": 1
            },
            "waves": [["pkg"]],
            "sectors": [{
                "id": "pkg", "path": "pkg", "files": ["pkg/a.py"],
                "bytes": 100, "lines": 10, "depends_on": [], "oversized": false,
                "units": [{
                    "rel": "pkg/a.py", "bytes": 100, "line_start": 1,
                    "line_end": 10, "chunk": 1, "chunks": 1
                }]
            }]
        });
        let plan = SectorPlan::from_json(&v).expect("an older plan must still load");
        assert_eq!(plan.totals.outside_root_files, 0);
        assert!(plan.totals.root_declined.is_empty());
        // Absent means "no scope accounting was recorded", which must not be
        // reported as "this plan dropped everything".
        assert_eq!(plan.totals.root_coverage(), 1.0);
        assert!(!plan.totals.feasibility().contains("SCOPE WARNING"));

        // And a plan that *did* record a partial scope keeps it.
        v["totals"]["outside_root_files"] = serde_json::json!(3);
        v["totals"]["outside_root_bytes"] = serde_json::json!(300);
        v["totals"]["outside_root_examples"] = serde_json::json!(["tools/x.py (0 KB)"]);
        let partial = SectorPlan::from_json(&v).unwrap();
        assert_eq!(partial.totals.outside_root_files, 3);
        assert!((partial.totals.root_coverage() - 0.25).abs() < 1e-9);
        assert!(partial.totals.feasibility().contains("SCOPE WARNING"));
    }

    #[test]
    fn a_file_bigger_than_the_unit_ceiling_never_produces_a_zero_line_span() {
        let d = tmp();
        let root = d.path();
        write(root, "pkg/tiny.py", 500);
        let opts = SectorOptions {
            max_unit_bytes: 100,
            ..Default::default()
        };
        let plan = plan_sectors_scoped(root, &opts, 1);
        for u in &plan.sectors[0].units {
            assert!(u.line_end >= u.line_start, "{u:?}");
            assert!(u.line_start >= 1, "{u:?}");
        }
    }

    #[test]
    fn no_sector_id_carries_a_path_separator() {
        let d = tmp();
        let root = d.path();
        // Both id paths: one from a directory split, one from loose-file
        // bin-packing. The second is the one that used to leak `pkg/alpha-root`.
        for i in 0..6 {
            write(root, &format!("pkg/alpha/mod{i}.py"), 3_000);
        }
        write(root, "pkg/other/mod.py", 500);
        let opts = SectorOptions {
            max_sector_bytes: 10_000,
            ..Default::default()
        };
        let plan = plan_sectors_scoped(root, &opts, 1);
        assert!(plan.sectors.len() >= 3, "{}", plan.sectors.len());
        for s in &plan.sectors {
            assert!(!s.id.contains('/'), "unslugged id: {}", s.id);
            assert!(!s.id.is_empty());
        }
        let mut ids: Vec<&str> = plan.sectors.iter().map(|s| s.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate ids: {ids:?}");
    }

    #[test]
    fn every_file_lands_in_exactly_one_sector() {
        let d = tmp();
        let root = d.path();
        for sub in ["a", "b/c", "b/d"] {
            for i in 0..5 {
                write(root, &format!("pkg/{sub}/m{i}.py"), 5_000);
            }
        }
        write(root, "pkg/loose.py", 5_000);
        let opts = SectorOptions {
            max_sector_bytes: 12_000,
            max_files_per_sector: 3,
            ..Default::default()
        };
        let plan = plan_sectors_scoped(root, &opts, 1);
        let collected = collect_source_files(root, &opts);
        let mut in_sectors: Vec<String> = plan
            .sectors
            .iter()
            .flat_map(|s| s.files.iter().cloned())
            .collect();
        in_sectors.sort();
        let mut expected: Vec<String> = collected
            .iter()
            .filter(|f| f.rel.starts_with("pkg/"))
            .map(|f| f.rel.clone())
            .collect();
        expected.sort();
        assert_eq!(in_sectors, expected, "sectors must partition the source");
        // And the wave schedule must cover every sector exactly once.
        let mut in_waves: Vec<String> = plan.waves.concat();
        in_waves.sort();
        let mut sector_ids: Vec<String> = plan.sectors.iter().map(|s| s.id.clone()).collect();
        sector_ids.sort();
        assert_eq!(
            in_waves, sector_ids,
            "waves must schedule every sector once"
        );
    }

    /// Manual scale check against a real, large repository.
    ///
    /// Ignored by default because the test suite cannot ship a 200 MB clone.
    /// Point `VELOCITY_SECTOR_SCALE_REPO` at one and run:
    /// `cargo test -p velocity_mcp --lib sectors::tests::scale_check -- --ignored --nocapture`
    /// It asserts only the invariants that must hold at any scale, and prints
    /// the numbers, so a decomposition that quietly degrades on a real tree
    /// (one giant sector, every sector in one wave) is visible.
    #[test]
    #[ignore = "set VELOCITY_SECTOR_SCALE_REPO to a large clone and pass --ignored"]
    fn scale_check_against_a_real_repository() {
        let Some(repo) = std::env::var("VELOCITY_SECTOR_SCALE_REPO")
            .ok()
            .filter(|s| !s.trim().is_empty())
        else {
            eprintln!("VELOCITY_SECTOR_SCALE_REPO not set; skipping");
            return;
        };
        let root = Path::new(&repo);
        assert!(root.is_dir(), "{} is not a directory", repo);
        let started = std::time::Instant::now();
        let opts = SectorOptions {
            // Optional, so the same test measures both halves of the scope
            // decision: detection, which refuses a root that would drop code,
            // and an operator's deliberate narrowing, which is taken as given
            // and only has to report what it costs. Those are different code
            // paths through `plan_sectors_inner` and both need a real number.
            source_root: std::env::var("VELOCITY_SECTOR_SCALE_ROOT").unwrap_or_default(),
            ..Default::default()
        };
        let plan = plan_sectors_scoped(root, &opts, 8);
        let elapsed = started.elapsed();
        let t = &plan.totals;
        eprintln!("repo            : {}", repo);
        eprintln!("requested root  : '{}'", opts.source_root);
        eprintln!("source_root     : {}", plan.source_root);
        eprintln!("decompose time  : {elapsed:?}");
        eprintln!("files/bytes/lines: {} / {} / {}", t.files, t.bytes, t.lines);
        eprintln!("sectors         : {}", t.sectors);
        eprintln!("dispatches      : {}", t.dispatches);
        eprintln!("waves           : {} (largest {})", t.waves, t.largest_wave);
        eprintln!("over budget     : {}", t.sectors_over_budget);
        eprintln!(
            "chunked files   : {} -> {} chunks",
            t.files_chunked, t.chunks
        );
        eprintln!("units over ceil : {}", t.units_over_ceiling);
        eprintln!("cycles broken   : {}", t.cycles_broken);
        eprintln!(
            "generated skip  : {} files, {} bytes ({} KB)",
            t.generated_skipped,
            t.generated_bytes,
            t.generated_bytes / 1024
        );
        for ex in &t.generated_examples {
            eprintln!("                  - {ex}");
        }
        eprintln!("root coverage   : {:.1}%", t.root_coverage() * 100.0);
        if t.outside_root_files > 0 {
            eprintln!(
                "outside root    : {} files, {} bytes ({} KB) NOT SCHEDULED",
                t.outside_root_files,
                t.outside_root_bytes,
                t.outside_root_bytes / 1024
            );
            for ex in &t.outside_root_examples {
                eprintln!("                  - {ex}");
            }
        }
        if !t.root_declined.is_empty() {
            eprintln!("root declined   : {}", t.root_declined);
        }
        eprintln!(
            "estimate        : {} model-secs = {:.1} model-hours; {:.1} wall-hours at {}x",
            t.est_model_secs,
            t.est_model_secs as f64 / 3600.0,
            t.est_wall_hours_at_parallelism,
            t.parallelism
        );
        // The operator-facing sentence, printed verbatim: it is the thing that
        // actually gets read, and a scope caveat that renders wrongly here is
        // invisible in the raw numbers above.
        eprintln!("feasibility     : {}", t.feasibility());
        let biggest = plan.sectors.iter().max_by_key(|s| s.bytes).map(|s| {
            (
                s.bytes,
                s.id.clone(),
                s.files.len(),
                s.units.len(),
                s.oversized,
            )
        });
        if let Some((bytes, id, files, units, oversized)) = biggest {
            eprintln!(
                "largest sector  : {id} ({bytes} bytes, {files} files, {units} units, oversized={oversized})"
            );
        }

        assert!(
            t.sectors > 1,
            "a large repo must decompose into many sectors"
        );
        assert_eq!(
            t.dispatches,
            plan.sectors.iter().map(|s| s.units.len()).sum::<usize>(),
            "dispatch count must equal the sum of unit counts"
        );
        let scheduled: usize = plan.waves.iter().map(|w| w.len()).sum();
        assert_eq!(scheduled, t.sectors, "every sector must be scheduled");
        assert!(
            t.largest_wave < t.sectors,
            "all {} sectors in one wave means the dependency scan found nothing",
            t.sectors
        );
        let mut ids: Vec<&str> = plan.sectors.iter().map(|s| s.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "sector ids must be unique at scale");
        for s in &plan.sectors {
            assert!(!s.units.is_empty(), "sector {} has no units", s.id);
            for u in &s.units {
                assert!(u.line_end >= u.line_start, "bad span {u:?}");
            }
        }
        // An over-ceiling unit is legitimate only when it is a single line:
        // chunking splits at line boundaries, so one line longer than the
        // ceiling cannot be split at all and has to be reported rather than
        // silently truncated. A *multi-line* unit over the ceiling is the
        // chunker giving up, which is the defect this assertion exists to catch.
        let mut over: Vec<&SectorUnit> = Vec::new();
        for s in &plan.sectors {
            for u in &s.units {
                if u.bytes > opts.max_unit_bytes {
                    over.push(u);
                }
            }
        }
        assert_eq!(
            over.len(),
            t.units_over_ceiling,
            "the reported over-ceiling count must match the units themselves"
        );
        for u in &over {
            eprintln!(
                "unsplittable    : {} ({} bytes on one line)",
                u.rel, u.bytes
            );
            assert_eq!(
                u.line_start, u.line_end,
                "a multi-line unit over the ceiling means the chunker gave up: {u:?}"
            );
        }
        // The accounting identity. Re-walks the tree independently of the plan
        // and requires that every collected file and byte is either scheduled or
        // counted as dropped — there is no third outcome. This is what makes
        // `totals.bytes` mean something at scale instead of being a number that
        // happens to be in the report.
        //
        // It does not cover exclusions made by directory name in the collector,
        // which happen upstream of this arithmetic; see `NON_SOURCE_DIRS`.
        let collected = collect_source_files(root, &opts);
        let collected_bytes: u64 = collected.iter().map(|f| f.bytes).sum();
        assert_eq!(
            t.bytes + t.outside_root_bytes,
            collected_bytes,
            "scheduled bytes + dropped bytes must account for every collected byte"
        );
        assert_eq!(
            t.files + t.outside_root_files,
            collected.len(),
            "scheduled files + dropped files must account for every collected file"
        );
        assert!(
            t.root_coverage() > 0.0,
            "a plan that schedules nothing is not a decomposition of {repo}"
        );

        // Conditional on the tree being big enough to plausibly vendor
        // something. This used to be unconditional, and it failed on
        // pypa/packaging — 480 KB of clean hand-written library that genuinely
        // excludes nothing as generated. The minified-file rule has its own unit
        // test (`a_minified_bundle_is_excluded_as_generated`); duplicating it
        // here over whatever repository an operator points the scale check at
        // asserts something about the repository rather than about this code.
        if collected_bytes > 5_000_000 {
            assert!(
                t.generated_skipped > 0,
                "a {collected_bytes}-byte tree vendors something; if nothing was excluded the \
                 minified-file rule is not firing"
            );
        }
    }

    #[test]
    fn a_minified_bundle_is_excluded_as_generated() {
        let d = tmp();
        let root = d.path();
        // Hand-written source: many short lines.
        write(root, "pkg/app.js", 30_000);
        // Vendored bundle: the same order of size on two lines. LiteLLM ships
        // exactly this as litellm/proxy/swagger/swagger-ui-bundle.js
        // (1.45 MB, 2 lines).
        //
        // Under `swagger`, not `vendor`: a directory named `vendor` is dropped
        // by [`SKIP_DIRS`] before any file in it is read, so putting the bundle
        // there tests the skip list instead of the minified-file rule and the
        // assertion below passes for the wrong reason.
        let dir = root.join("pkg/swagger");
        fs::create_dir_all(&dir).unwrap();
        let mut minified = String::new();
        minified.push_str(&"x".repeat(40_000));
        minified.push('\n');
        minified.push_str(&"y".repeat(40_000));
        fs::write(dir.join("bundle.js"), &minified).unwrap();

        let (files, skipped) = collect_source_files_reported(root, &SectorOptions::default());
        let rels: Vec<&str> = files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, vec!["pkg/app.js"], "{rels:?}");
        assert_eq!(skipped.files, 1);
        assert!(skipped.bytes >= 80_000, "{}", skipped.bytes);
        assert_eq!(skipped.examples.len(), 1);
        assert!(
            skipped.examples[0].contains("bundle.js"),
            "{}",
            skipped.examples[0]
        );
    }

    /// The `schema.d.ts` regression: generated output a person can read.
    ///
    /// LiteLLM's `ui/litellm-dashboard/src/lib/http/schema.d.ts` is 3 MB emitted
    /// by `openapi-typescript` — 84,560 lines averaging 36 characters, under a
    /// header saying so in terms. Neither of the other two rules can see it: the
    /// name is not a build-output shape, because `.d.ts` is equally what a
    /// hand-written declaration file is called, and the line shape is ordinary
    /// legible source rather than a minified blob. It was scheduled as 128
    /// porting dispatches, roughly 10.7 model-hours and the single largest
    /// sector in the plan, to hand-translate a type surface that regenerates
    /// from an OpenAPI spec on demand.
    #[test]
    fn a_legible_generated_declaration_file_is_excluded_by_its_own_banner() {
        let d = tmp();
        let root = d.path();
        fs::create_dir_all(root.join("ui/lib/http")).unwrap();
        // The real header verbatim, then declarations at the real average line
        // length, so the rule that fires here cannot be the minified one.
        let mut body = String::from(
            "/**\n * This file was auto-generated by openapi-typescript.\n * Do not make direct changes to the file.\n */\n\n",
        );
        while body.len() < 40_000 {
            body.push_str("    statusCode: number;\n");
        }
        fs::write(root.join("ui/lib/http/schema.d.ts"), &body).unwrap();
        write(root, "ui/lib/handwritten.ts", 30_000);

        let (files, skipped) = collect_source_files_reported(root, &SectorOptions::default());
        let rels: Vec<&str> = files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, vec!["ui/lib/handwritten.ts"], "{rels:?}");
        assert_eq!(skipped.files, 1, "{skipped:?}");
        assert!(skipped.bytes >= 40_000, "{}", skipped.bytes);
        // The reason has to survive into the report. An exclusion an operator
        // cannot audit is the same defect as no exclusion at all, only quieter.
        assert!(
            skipped.examples[0].contains("schema.d.ts"),
            "{}",
            skipped.examples[0]
        );
        assert!(
            skipped.examples[0].contains("auto-generated by openapi-typescript"),
            "the banner is the evidence: {}",
            skipped.examples[0]
        );
    }

    /// The other side of the same rule: a hand-written declaration file has the
    /// identical name and must survive. This is why the exclusion keys on the
    /// banner the generator wrote rather than on `.d.ts`.
    #[test]
    fn a_hand_written_declaration_file_is_kept() {
        let d = tmp();
        let root = d.path();
        fs::create_dir_all(root.join("pkg")).unwrap();
        fs::write(
            root.join("pkg/api.d.ts"),
            "import type { Config } from './config';\n\nexport declare function load(path: string): Config;\n",
        )
        .unwrap();

        let (files, skipped) = collect_source_files_reported(root, &SectorOptions::default());
        let rels: Vec<&str> = files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, vec!["pkg/api.d.ts"], "{rels:?}");
        assert_eq!(skipped.files, 0, "{skipped:?}");
    }

    /// Provenance has nothing to do with size, and neither does the rule.
    ///
    /// This file is far below `max_unit_bytes`, so before the header sniff it
    /// was never opened at all and a codegen stub of any size was scheduled as
    /// one dispatch of real work.
    #[test]
    fn the_banner_rule_applies_to_small_files_too() {
        let d = tmp();
        let root = d.path();
        fs::create_dir_all(root.join("pkg")).unwrap();
        fs::write(
            root.join("pkg/gen_pb2.py"),
            "# @generated by protoc-gen-python. DO NOT EDIT.\nimport sys\n",
        )
        .unwrap();
        write(root, "pkg/real.py", 800);

        let (files, skipped) = collect_source_files_reported(root, &SectorOptions::default());
        let rels: Vec<&str> = files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, vec!["pkg/real.py"], "{rels:?}");
        assert!(
            skipped.examples[0].contains("header:"),
            "the example must say which rule fired: {}",
            skipped.examples[0]
        );
    }

    /// The sniff window is a header, not the whole file.
    ///
    /// Otherwise any source file that mentions a generator anywhere in its body
    /// — a docstring explaining how fixtures were produced, a note about a
    /// codegen step in CI — would be excluded, and losing real code from a port
    /// is a much worse failure than scheduling somebody's build output.
    #[test]
    fn a_mention_of_generation_deep_in_a_file_does_not_exclude_it() {
        let d = tmp();
        let root = d.path();
        fs::create_dir_all(root.join("pkg")).unwrap();
        let mut body = String::from("import os\n\n\ndef load():\n");
        while body.len() < HEADER_SNIFF_BYTES {
            body.push_str("    pass\n");
        }
        body.push_str("    # fixtures below are auto-generated by conftest\n");
        fs::write(root.join("pkg/deep.py"), &body).unwrap();

        let (files, skipped) = collect_source_files_reported(root, &SectorOptions::default());
        assert_eq!(files.len(), 1, "a banner has to be in the header to count");
        assert_eq!(skipped.files, 0, "{skipped:?}");
    }

    /// Prose *about* a generator, in a header, is not a banner.
    ///
    /// Both files here are verbatim from `litellm-rust/crates/python-compat/`,
    /// the first repository the header rule ran against. A `"generated by"`
    /// marker excluded them both, and both are hand-written source: a crate
    /// root whose doc explains where its test fixtures come from, and a module
    /// that documents the generated file it `include!`s. The distinction from
    /// [`a_legible_generated_declaration_file_is_excluded_by_its_own_banner`]
    /// is the subject of the sentence — a real banner says the *file* was
    /// generated, and every remaining marker requires that.
    #[test]
    fn prose_about_a_generator_in_a_header_is_not_a_banner() {
        let d = tmp();
        let root = d.path();
        fs::create_dir_all(root.join("crates/python-compat/src")).unwrap();
        fs::write(
            root.join("crates/python-compat/src/lib.rs"),
            "//! Python data formats reproduced in Rust.\n\
             //!\n\
             //! Each module mirrors one Python operation, and its tests replay\n\
             //! fixtures generated by that operation in CPython (`scripts/generate_fixtures.py`):\n\
             //!\n\
             //! | Module | Python operation |\n\n\
             pub mod repr;\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/python-compat/src/repr.rs"),
            "//! `repr(value)` byte for byte.\n\n\
             /// Generated by `scripts/generate_nonprintable.py`; see `generated/nonprintable.rs`.\n\
             mod nonprintable {}\n\n\
             pub fn repr(value: &str) -> String {\n    value.to_string()\n}\n",
        )
        .unwrap();

        let (files, skipped) = collect_source_files_reported(root, &SectorOptions::default());
        let rels: Vec<&str> = files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(
            rels,
            vec![
                "crates/python-compat/src/lib.rs",
                "crates/python-compat/src/repr.rs"
            ],
            "{rels:?}"
        );
        assert_eq!(skipped.files, 0, "{skipped:?}");
    }

    /// The list is only worth what its weakest entry catches, so every phrasing
    /// in it has to fire. These are the conventions real generators emit —
    /// Buck/Thrift, Go, protoc, openapi-typescript — and an entry that silently
    /// stopped matching would otherwise only show up as a plan that got bigger.
    #[test]
    fn every_banner_phrasing_in_the_list_is_recognised() {
        for marker in GENERATED_HEADER_MARKERS {
            let head = format!("/* {marker} some-tool v2 */\nexport const x = 1;\n");
            let banner = generated_banner(head.as_bytes())
                .unwrap_or_else(|| panic!("no banner recognised for {marker:?}"));
            assert!(banner.contains(marker), "{marker:?} -> {banner}");
            assert!(
                !banner.contains('/') && !banner.contains('*'),
                "comment furniture should be stripped: {banner}"
            );
        }
    }

    /// A banner is quoted in a report, so it is capped — and the cap has to land
    /// on a character boundary, because these come from repositories written in
    /// any language and slicing UTF-8 by byte index panics.
    #[test]
    fn a_long_banner_is_truncated_without_splitting_a_character() {
        let head = format!("// @generated {}\n", "ü".repeat(200));
        let banner = generated_banner(head.as_bytes()).expect("banner");
        assert!(banner.starts_with("@generated"), "{banner}");
        assert!(banner.ends_with('…'), "{banner}");
        assert!(
            banner.len() < head.len(),
            "truncation should shorten it: {} vs {}",
            banner.len(),
            head.len()
        );
        // The cap is on the quoted text, so the ellipsis is the one extra char.
        assert!(
            banner.chars().count() <= 121,
            "{} chars: {banner}",
            banner.chars().count()
        );
    }

    /// A committed frontend build tree is not portable source.
    ///
    /// Regression for what the LiteLLM scale check found: 311 files of Next.js
    /// output under `litellm/proxy/_experimental/out/_next/static/` were being
    /// scheduled as source, and two of them carried single lines long enough to
    /// defeat chunking entirely. The line-shape rule cannot catch these — a
    /// chunked bundle has plenty of short lines, so its *average* looks like
    /// source — which is why the directory and filename rules exist.
    #[test]
    fn frontend_build_output_is_not_treated_as_source() {
        let d = tmp();
        let root = d.path();
        write(root, "pkg/app.tsx", 4_000);
        // Emitted asset tree, full of ordinary-looking short lines.
        write(root, "pkg/out/_next/static/chunks/main.js", 40_000);
        write(root, "pkg/out/_next/static/media/logo.js", 4_000);
        // Bundles committed straight into the source tree.
        write(root, "pkg/assets/vendor.min.js", 40_000);
        write(root, "pkg/assets/site.min.css", 40_000);
        write(root, "pkg/assets/app.bundle.js", 40_000);

        let (files, skipped) = collect_source_files_reported(root, &SectorOptions::default());
        let rels: Vec<&str> = files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, vec!["pkg/app.tsx"], "{rels:?}");
        assert_eq!(skipped.files, 3, "the three named bundles: {skipped:?}");
        assert!(
            skipped
                .examples
                .iter()
                .any(|e| e.contains("vendor.min.js") && e.contains("generated filename")),
            "{:?}",
            skipped.examples
        );
    }

    #[test]
    fn a_hand_written_file_with_one_long_line_is_kept() {
        let d = tmp();
        let root = d.path();
        fs::create_dir_all(root.join("pkg")).unwrap();
        fs::write(root.join("pkg/mixed.py"), uneven_source()).unwrap();
        // 12 KB across 401 lines: average line length 27, so this is source,
        // not a blob, despite containing a single 9 KB line.
        let opts = SectorOptions {
            max_unit_bytes: 2_000,
            ..Default::default()
        };
        let files = collect_source_files(root, &opts);
        assert_eq!(files.len(), 1, "one long line must not condemn a file");
        assert_eq!(files[0].lines, 401, "{}", files[0].lines);
    }

    #[test]
    fn chunking_respects_the_ceiling_and_reports_the_line_it_cannot_split() {
        let d = tmp();
        let root = d.path();
        fs::create_dir_all(root.join("pkg")).unwrap();
        fs::write(root.join("pkg/uneven.py"), uneven_source()).unwrap();
        let opts = SectorOptions {
            max_unit_bytes: 2_000,
            ..Default::default()
        };
        let plan = plan_sectors_scoped(root, &opts, 1);
        let units = &plan.sectors[0].units;
        assert!(
            units.len() > 1,
            "a 12 KB file over a 2 KB ceiling must split"
        );
        let mut over = 0usize;
        for u in units {
            if u.bytes > opts.max_unit_bytes {
                over += 1;
                // The only legitimate exception is a single line longer than the
                // ceiling, which cannot be split at a line boundary at all.
                assert_eq!(
                    u.line_start, u.line_end,
                    "a multi-line chunk must respect the ceiling: {u:?}"
                );
            }
        }
        assert_eq!(over, 1, "exactly one 9 KB line is unsplittable: {units:?}");
        assert_eq!(plan.totals.units_over_ceiling, 1);
        // Chunks must tile the file with no gaps and no overlap.
        assert_eq!(units[0].line_start, 1);
        for w in units.windows(2) {
            assert_eq!(
                w[0].line_end + 1,
                w[1].line_start,
                "gap or overlap between chunks {:?} and {:?}",
                w[0],
                w[1]
            );
        }
        assert_eq!(units.last().unwrap().line_end, 401);
        assert!(units.iter().all(|u| u.chunks == units.len() as u32));
    }

    /// 200 short lines, one 9 KB line, 200 short lines. Realistic worst case for
    /// line-based chunking: the even line-count division would put hundreds of
    /// short lines and the 9 KB line in the same chunk.
    fn uneven_source() -> String {
        let mut body = String::new();
        for i in 0..200 {
            body.push_str(&format!("x{i}\n"));
        }
        body.push_str(&"z".repeat(9_000));
        body.push('\n');
        for i in 0..200 {
            body.push_str(&format!("y{i}\n"));
        }
        body
    }
}
