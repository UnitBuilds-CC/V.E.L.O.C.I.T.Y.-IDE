import {
  Callout,
  Divider,
  Grid,
  H1,
  H2,
  Stack,
  Stat,
  Table,
  Tag,
  Text,
} from 'qoder/canvas';

const coverageTiers = [
  {
    tier: 'A',
    label: 'Record in .velocity/events/events.jsonl',
    meaning: 'dispatch actually ran the tool and wrote a verdict',
    count: '252 tools',
  },
  {
    tier: 'B',
    label: 'Verdict printed in a run log, never recorded',
    meaning: 'a real stdio client saw the answer but dispatch did not log it',
    count: '0 tools',
  },
  {
    tier: 'C',
    label: 'No evidence of any kind',
    meaning: 'never executed: obsolete surface, gated, or declined',
    count: '14 tools',
  },
];

const remainingGap = [
  {
    group: 'runtime_* (8 never executed, 4 executed and failing) + browser_runtime_* (2)',
    why:
      'Obsolete surface, not a testing gap. These 14 are the client half of mcp-lite - the earlier Go service that drove Chrome through chromedp. That backend is gone and has been superseded: the internal browser is now written in Rust in velocity-browser, and its capability reaches the agent twice over as browser_native_* (the native engine) and browser_session_*/browser_* (the persisted engine). Nothing in this repo serves the http://127.0.0.1:8080/api/runtime/* routes these tools still resolve, and there is no Go source at all.',
    closes:
      'Delete the 14, or re-point them at the in-process Rust engine. Either way their advertised text needs correcting first: 13 of the 14 still tell the model they use a "Go chromedp runtime" or "Go browser API", which is how an agent ends up calling a retired backend.',
  },
  {
    group: 'drone_click, drone_type_keys, drone_deploy (3 of the 10 drone_* tools)',
    why:
      'Declined on purpose, after the other 7 were run against a drone started locally. The first two synthesise keyboard and mouse input into this machine; the third SSHes to a remote host and copies a build onto it. All three are also inert as shipped: they go through POST /peer/message, which the drone files in a bounded queue and never dispatches on.',
    closes:
      'Nothing to measure until the drone exposes an endpoint that reaches drone/src/system.rs. Input synthesis on your live desktop needs your consent regardless.',
  },
  {
    group: 'wa_vdesktop_create, wa_vdesktop_remove, wa_file_dialog_save (3)',
    why:
      'Left unexecuted on purpose. These move or destroy the operator session, and you answered "Leave them gated".',
    closes: 'Nothing - requires your consent',
  },
];

const findingThemes = [
  {
    theme: 'Claimed success for work that never happened',
    examples:
      '#7 generate_wiki reported a finished wiki over 0 file pages; #38/#39 wa_tray_click and wa_window_action answered success:false while the detail text said executed; #11 execute_nda hung a sidecar chain and still looked fine',
  },
  {
    theme: 'Errors unreadable to the agent that has to act on them',
    examples:
      '#33 empty script output surfaced as a serde EOF leak; #34 and #55 raw "os error 2" with no idea which page or session was missing; #56 a ureq Transport struct dumped into the message; #41 arguments recorded unredacted',
  },
  {
    theme: 'Schema documentation that did not match behaviour',
    examples:
      '#23 wa_screenshot format and save_to; wa_window_tile promised "2-column, 3-column, or custom" with no columns argument; #57 browser_session_fill advertises matching by "field label, name, or nearby semantic text" while only aria-label worked',
  },
  {
    theme: 'State that did not persist or round-trip',
    examples:
      '#2/#6 agent checkpoints written then lost; #9 a provider label that survived the UI but not the restore; #10 a team export that could not be re-imported',
  },
  {
    theme: 'Consent and safety',
    examples:
      '#40/#40b default-deny gate over desktop switching, clipboard writes and process launches - the reason the sweep used to cover your screen in notepad windows',
  },
  {
    theme: 'Two halves speaking different contracts',
    examples:
      '#59-#62 the MCP drone client POSTed command, upload_id and chunk_index; the drone reads instructions, transfer_id and index, and defaults a missing key instead of rejecting it, so drone_command ran an empty string and reported success while drone_upload could not start. #64 is the same shape inside one process: a capture filter that matched nothing was dropped rather than refused, and the foreground window was captured instead.',
  },
];

const openIssues = [
  {
    item: '14 tools advertise mcp-lite\u2019s retired Go/Chrome backend',
    detail:
      'runtime_create_session, runtime_close_session, runtime_get_session, runtime_capture_session, runtime_reseed_auth and the seven runtime_session_* actions, plus browser_runtime_capture and browser_runtime_visual_capture, all resolve http://127.0.0.1:8080 as a "Go browser API". That was mcp-lite, which was written in Go and drove Chrome through chromedp. It has been superseded: velocity-browser is the in-house Rust engine, and it reaches the agent as browser_native_* and browser_session_*. There is no Go source in the repo and nothing serves those routes, so 13 of the 14 still describe a backend that no longer exists and the other 1 implies it. They fail honestly rather than lying, but they are 5% of the advertised surface an agent chooses from, permanently unreachable.',
    tone: 'danger' as const,
  },
  {
    item: 'The drone runs shell commands with no authentication, on any interface',
    detail:
      'POST /peer/task executes cmd /C {instructions} with no allowlist and no bearer token unless DRONE_AUTH_TOKEN happens to be set. There is no loopback check before TcpListener::bind and the binary\u2019s own --help advertises --host 0.0.0.0. Measured, not inferred: an unauthenticated local client ran a command and got its stdout back with zero credentials.',
    tone: 'danger' as const,
  },
  {
    item: '--capabilities authorises nothing',
    detail:
      'The drone takes --capabilities metadata_only and echoes it in the startup banner and its identity JSON. No request path reads it. A peer that asks for metadata-only behaviour gets cmd /C.',
    tone: 'danger' as const,
  },
  {
    item: 'Upload deploy lines allowlist run but not copy',
    detail:
      'execute_deploy_instructions checks run commands against an allowlist and passes copy {file} <dest> straight through, so an upload can write any path on the drone. Left unfixed deliberately: the documented example in DRONE_PROTOCOL.md is copy {file} /opt/apps/latest.exe, i.e. outside the workspace, so restricting it is a product decision rather than a bug fix.',
    tone: 'warning' as const,
  },
  {
    item: 'Drone identity silently discards its CLI flags',
    detail:
      'Bug #63, reported not fixed. DroneIdentity::load_or_create restores name and capabilities from .velocity/drone_identity.json and refreshes only the port and start time, so --name and --capabilities are ignored on every start after the first in a workspace. Observed: started with --name TestDrone, the banner printed sweep-drone. The persisted id never rotates either, so two clones of a workspace come up as the same drone.',
    tone: 'warning' as const,
  },
  {
    item: '38 KB of drone capability nothing can call',
    detail:
      'drone/src/system.rs (GDI screen capture, SendInput click and type, a network monitor) and drone/src/scheduler.rs (priorities, progress, cancel) have zero references outside their own modules. That is why drone_screenshot, drone_click and drone_type_keys can only ever return a receipt: wiring these to an endpoint is a feature, not a fix.',
    tone: 'warning' as const,
  },
  {
    item: 'Audit record cannot express intent',
    detail:
      'events.jsonl keys are affected_files, context, description, failure_reason, merkle_root_*, outcome, sequence, timestamp_ms, tool_name. There is no expected-failure field, so the 50 tools whose last recorded call errored are not 50 defects: most are negative probes where the error was the point (a dead port, a fake selector, a consent refusal, an upload given an rm action). Reading that column as health is how I mis-reported my own coverage twice.',
    tone: 'warning' as const,
  },
  {
    item: 'Two Rust browser engines that drift',
    detail:
      'velocity-browser::BrowserSession drives browser_native_*; a separate ureq-backed engine inside velocity-mcp drives browser_session_* and browser_*. Bugs #45/#46 were fixed only on the native side, which is precisely how #56/#57/#58 came to exist on the other one. Both are in-house now; the third path, the Go/chromedp one, was retired without deleting its client.',
    tone: 'danger' as const,
  },
  {
    item: 'parse_forms ignores <select>',
    detail:
      'Dropdowns are not extracted at all, so a select cannot be filled by any label-matching strategy. Found while reading the code for #57, not yet fixed.',
    tone: 'warning' as const,
  },
  {
    item: 'code_generate_tests is template-only',
    detail:
      'The name implies an LLM call. It does not make one.',
    tone: 'warning' as const,
  },
  {
    item: 'JS interpreter gaps in the native engine',
    detail:
      'browser_native_eval resolves window.location but document.querySelectorAll returns Undefined, so DOM assertions have to go through the snapshot tools instead.',
    tone: 'warning' as const,
  },
  {
    item: 'A refused desktop probe records your window titles',
    detail:
      'Since #64, a capture that matches no window answers by listing the live windows so the agent can retry with a real id. That text lands in failure_reason in the append-only audit log, capped at 12 entries. It is the same information wa_window_list returns on demand and the log is local to your workspace, but it is a deliberate trade-off rather than an accident, so it is worth a decision.',
    tone: 'info' as const,
  },
];

export default function McpToolSweepReport() {
  return (
    <Stack gap={20}>
      <H1>Velocity MCP Tool Sweep - Usability, Functionality, Effectiveness</H1>

      <Callout tone="warning">
        <Text>
          Not finished. 252 of 266 advertised tools have durable evidence that they were actually
          executed by a real stdio client. Of the 14 that remain, 8 are the obsolete mcp-lite surface
          - the runtime_* family still calls the retired Go/Chrome backend that the Rust browser
          replaced - 3 need your consent because they act on the live desktop, and 3 are drone probes
          that synthesise input or SSH outbound. 64 findings were raised along the way and 62 of them
          held up when re-tested.
        </Text>
      </Callout>

      <Grid columns={4} gap={16}>
        <Stat value="252/266" label="Tools with execution evidence" tone="warning" />
        <Stat value="64" label="Findings raised (#1-#64)" />
        <Stat value="4,714" label="Unit/integration tests, 0 failing" tone="success" />
        <Stat value="18.7%" label="Historical failure rate (resolved calls)" />
      </Grid>

      <Divider />

      <H2>Why this number moved, and why you should trust it</H2>
      <Stack gap={8}>
        <Text>
          This report has now corrected its own coverage figure four times, and the first three
          were wrong in the same direction: counting a written probe as a run one. The claim was
          250/266 with a gap of 16; reconciled against the audit store it was 211. Then batches
          16, 16b and 16c took it to 246, and running the drone family plus one forgotten
          Windows-automation tool took it to 252.
        </Text>
        <Text>
          The concrete case for not trusting spec files: sweep-b.jsonl - 79 probes covering the
          persisted-session browser tools, checkpoints, auth profiles, storage and workflow reads -
          was written and never run. It even references browser_navigate, which is not an advertised
          tool, and its line 8 launches notepad.exe. The lesson is structural rather than personal,
          so every figure in this report now comes from report_numbers.py re-reading the event store
          and the batch logs on the spot, not from notes.
        </Text>
        <Text>
          That script is also what caught the last two gaps: the drone family, reported as waiting
          on an external daemon when the daemon is a binary in this repo that starts locally, and
          wa_capture_windows_snapshot, whose only audit record was written an hour before the fix it
          was meant to verify was committed.
        </Text>
      </Stack>

      <Table
        headers={['Tier', 'Source', 'What it actually proves', 'Tools']}
        rows={coverageTiers.map((t) => [
          t.tier,
          t.label,
          t.meaning,
          t.count,
        ])}
        density="compact"
      />

      <Divider />

      <H2>Current state, per tool</H2>
      <Grid columns={3} gap={16}>
        <Stat value="153" label="Last recorded call succeeded" tone="success" />
        <Stat value="50" label="Last recorded call errored" tone="warning" />
        <Stat value="49" label="Last call never closed out" />
      </Grid>
      <Stack gap={8}>
        <Text>
          These are read from the audit store, which is append-only history rather than a status
          board, so each column needs its caveat stated rather than glossed. 1,151 calls are
          recorded in total, across the two workspaces the sweep ran in.
        </Text>
        <Stack gap={4}>
          <Row>
            <Tag tone="warning">Read with care</Tag>
            <Text size="small">
              The 50 include every probe that was designed to fail. drone_status and drone_upload
              are in that list because their last calls were a dead port, an absent local file and
              an unsupported deploy action - the probes passed by failing. browser_read_snapshot is
              there for the readable 'no snapshot artifact is stored for this URL' message that bug
              #55 added. There is no intent field, so the store cannot separate the two.
            </Text>
          </Row>
          <Row>
            <Tag>Explained</Tag>
            <Text size="small">
              The 49 pending records all predate the bug #13 fix that resolves a verdict in the
              same write. Pending stops dead at sequence 199; all 889 records after it are
              resolved, which is the live proof that #13 works.
            </Text>
          </Row>
        </Stack>
      </Stack>

      <Divider />

      <H2>What executing it found that reading the code did not</H2>
      <Text>
        62 of the 64 findings survived re-testing - #30 and #36 were mine and were wrong, so I
        withdrew them. Grouped by what kind of lie each one was:
      </Text>
      <Table
        headers={['Failure theme', 'Representative findings']}
        rows={findingThemes.map((f) => [f.theme, f.examples])}
        density="compact"
      />

      <Callout tone="info">
        <Text>
          The pattern across all five themes is the same, and it is the thing worth fixing on the
          roadmap: this surface is generous about answering when it did nothing, and terse when it
          did fail. A tool that cannot find its subject tends to return an error string written for
          the developer who threw it, not the agent that has to decide what to do next.
        </Text>
      </Callout>

      <Divider />

      <H2>Effectiveness</H2>
      <Stack gap={8}>
        <Text>
          The LLM-backed tools ran against qwen3.8-flash through the token plan. Where a tool is
          genuinely model-backed it produced usable output; where the name implied a model and none
          was called, that is recorded above as a finding rather than counted as a pass.
        </Text>
        <Text>
          The persisted browser session demonstrably works end to end: it navigates over real
          network, seals captures to NDA, checkpoints and restores, and persists auth profiles,
          run reports, transcripts and workflow files. Verified on disk rather than by trusting the
          tool's own reply - browser-auth-profiles, browser-runs, browser-session-checkpoints,
          browser-snapshots, browser-captures and browser-suites all held the expected artifacts
          after the batches finished.
        </Text>
        <Text>
          The drone works the same way once both halves agree on the field names, and it did not
          before: a submitted command now reaches a real shell and its stdout comes back
          ('echo velocity-sweep-probe-42' returned exactly that, under its own task id), and a
          786,456-byte file uploaded in three chunks lands on disk under the requested name with a
          SHA-256 identical to the source. Before the fix the command completed with empty stdout
          and every upload died at step one.
        </Text>
      </Stack>

      <Divider />

      <H2>Safety</H2>
      <Stack gap={8}>
        <Text>
          Everything here ran without touching your interactive desktop. The default-deny gate from
          bug #40/#40b covers desktop switching, clipboard writes and process launches; the batch 13
          probes confirmed the gate refuses, that read-only tools still work, and that the desktop
          count and GUID were identical before and after.
        </Text>
        <Text>
          Three tools stay unexecuted because you said to leave them gated. They are reported as a
          gap rather than run by quietly setting VELOCITY_WA_ALLOW_SESSION_EFFECT.
        </Text>
        <Text>
          The drone ran isolated: its own sandbox workspace, bound to 127.0.0.1, stopped and
          confirmed gone afterwards (no velocity-drone process, nothing listening on 9191). The two
          probes that would have synthesised mouse and keyboard input into your session were not
          executed even after it was proven they are inert, and neither was drone_deploy, which
          SSHes outbound. The one Windows-automation tool that was re-run, wa_capture_windows_snapshot,
          only walks a window's accessibility tree - it moves nothing, focuses nothing, types nothing.
        </Text>
      </Stack>

      <Divider />

      <H2>What is left</H2>
      <Text>
        The gap is not one thing. Only part of it is "not yet tested", and one part of it is not a
        testing problem at all:
      </Text>
      <Table
        headers={['Remaining tools', 'Why they are not evidenced', 'What closes it']}
        rows={remainingGap.map((r) => [r.group, r.why, r.closes])}
        density="compact"
      />

      <Divider />

      <H2>Known problems still open</H2>
      <Stack gap={8}>
        {openIssues.map((issue) => (
          <Callout key={issue.item} tone={issue.tone}>
            <Stack gap={4}>
              <Text weight="semibold">{issue.item}</Text>
              <Text size="small">{issue.detail}</Text>
            </Stack>
          </Callout>
        ))}
      </Stack>

      <Divider />

      <H2>Verification state of the current build</H2>
      <Grid columns={2} gap={16}>
        <Stack gap={6}>
          <Text weight="semibold">Gates</Text>
          <Row>
            <Tag tone="success">PASS</Tag>
            <Text size="small">velocity_mcp lib: 2026 passed / 0 failed / 10 ignored</Text>
          </Row>
          <Row>
            <Tag tone="success">PASS</Tag>
            <Text size="small">velocity-browser lib: 2688 passed / 0 failed / 3 ignored</Text>
          </Row>
          <Row>
            <Tag tone="success">PASS</Tag>
            <Text size="small">cargo fmt --all --check, rc 0</Text>
          </Row>
          <Row>
            <Tag tone="success">PASS</Tag>
            <Text size="small">cargo clippy -p velocity_mcp --all-targets -- -D warnings, clean</Text>
          </Row>
        </Stack>
        <Stack gap={6}>
          <Text weight="semibold">Live batches against the release binary</Text>
          <Row>
            <Tag tone="success">51/51</Tag>
            <Text size="small">batch16 persisted session (was 44/7 before #55-#58)</Text>
          </Row>
          <Row>
            <Tag tone="success">10/10</Tag>
            <Text size="small">batch16b synthetic URLs and network policy</Text>
          </Row>
          <Row>
            <Tag tone="success">7/7</Tag>
            <Text size="small">batch16c label matching</Text>
          </Row>
          <Row>
            <Tag tone="success">138/138</Tag>
            <Text size="small">batch15 all 48 browser_native_* tools</Text>
          </Row>
          <Row>
            <Tag tone="success">11/11</Tag>
            <Text size="small">
              batch17b the drone family against a live local drone (was 4 tools answering with
              nothing before #59-#62)
            </Text>
          </Row>
          <Row>
            <Tag tone="success">5/5</Tag>
            <Text size="small">
              batch18 wa_capture_windows_snapshot, including the unmatched-filter refusal that is
              bug #64
            </Text>
          </Row>
          <Row>
            <Tag tone="success">4/4</Tag>
            <Text size="small">
              the #[ignore]d drone integration tests, which had never once been runnable
            </Text>
          </Row>
        </Stack>
      </Grid>

      <Stack gap={4}>
        <Text size="small" tone="secondary">
          Suite flake found and fixed on the way: a_killed_child_reports_stopped_via_exit_code
          slept a fixed 400ms and then demanded the child read as stopped, which lost roughly one
          run in thirteen under full-suite parallelism. Polls to a 4s bound now - 18 consecutive
          clean full-suite runs after the change.
        </Text>
        <Text size="small" tone="secondary">
          Committed as af4ddea (drone #59-#62) and 80b2ac4 (wa capture #64); browser
          #55-#58 were eceb833 and the test race fix 7d7e7fc. Figures here come from
          .velocity_testing/report_numbers.py, which re-reads the event stores and the batch logs,
          not from session notes - the last two rounds of numbers did come from session notes and
          both were stale.
        </Text>
      </Stack>
    </Stack>
  );
}

function Row({ children }: { children: React.ReactNode }) {
  return <div style={{ display: 'flex', alignItems: 'flex-start', gap: 8 }}>{children}</div>;
}
