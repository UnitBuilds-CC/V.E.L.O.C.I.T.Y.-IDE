import {
  Divider,
  Grid,
  H1,
  H2,
  H3,
  Stack,
  Stat,
  Table,
  Text,
  Tag,
  Callout,
  Timeline,
  useHostTheme,
} from "qoder/canvas";

export default function CompletionReport() {
  const theme = useHostTheme();

  return (
    <Stack gap={24} style={{ padding: 24 }}>
      <Stack gap={8}>
        <H1>GUI Code Review — Completion Report</H1>
        <Text tone="secondary">
          Velocity IDE · Commit 3b0d557 · Pushed to master
        </Text>
      </Stack>

      <Callout tone="success">
        <Text>
          All 4 review issues resolved. Code is cleaner (net −146 lines), DRY
          (single panel-data implementation), and correct (proper Unicode,
          correct comment placement, no build/action confusion). All 895 tests
          pass with zero warnings.
        </Text>
      </Callout>

      <Divider />

      <Grid columns={4} gap={16}>
        <Stat value="4" label="Issues Fixed" tone="success" />
        <Stat value="43" label="Files Reviewed" />
        <Stat value="~1,695" label="Insertions" />
        <Stat value="~1,280" label="Deletions" />
      </Grid>

      <Divider />

      <H2>Issues Fixed</H2>

      <Stack gap={16}>
        <Stack gap={4}>
          <Stack gap={4} direction="row" style={{ alignItems: "center" }}>
            <Tag tone="danger">Bug</Tag>
            <H3 style={{ margin: 0 }}>Corrupted Unicode in Tab Labels</H3>
          </Stack>
          <Text tone="secondary">
            Mission Control tab labels displayed ? instead of em-dash
            characters. Replaced with proper Unicode escape sequences.
          </Text>
          <Text size="small" tone="secondary">
            File: render.rs — 2 lines changed
          </Text>
        </Stack>

        <Stack gap={4}>
          <Stack gap={4} direction="row" style={{ alignItems: "center" }}>
            <Tag tone="warning">Cleanup</Tag>
            <H3 style={{ margin: 0 }}>Orphaned Comment Relocated</H3>
          </Stack>
          <Text tone="secondary">
            A comment describing active_symbol logic was displaced during
            toolbar refactoring. Moved it back above its declaration.
          </Text>
          <Text size="small" tone="secondary">
            File: ui_render.rs — 4 lines changed
          </Text>
        </Stack>

        <Stack gap={4}>
          <Stack gap={4} direction="row" style={{ alignItems: "center" }}>
            <Tag tone="info">DRY</Tag>
            <H3 style={{ margin: 0 }}>
              Extracted Shared fetch_panel_data_value()
            </H3>
          </Stack>
          <Text tone="secondary">
            Both the MCP tool handler and agent thread had duplicated panel
            serialization logic. Extracted a shared function in system_tools.rs
            so both callers use one path.
          </Text>
          <Text size="small" tone="secondary">
            File: system_tools.rs — +21 lines net
          </Text>
        </Stack>

        <Stack gap={4}>
          <Stack gap={4} direction="row" style={{ alignItems: "center" }}>
            <Tag tone="danger">Bug</Tag>
            <H3 style={{ margin: 0 }}>
              Removed run_build from FetchPanelData
            </H3>
          </Stack>
          <Text tone="secondary">
            The FetchPanelData handler contained a run_build branch that
            duplicated the existing RunLocalBuild handler and could prematurely
            signal AgentFinished. Removed entirely.
          </Text>
          <Text size="small" tone="secondary">
            File: thread.rs — −174 lines
          </Text>
        </Stack>
      </Stack>

      <Divider />

      <H2>Changed Files</H2>
      <Table
        headers={["File", "Change", "Description"]}
        rows={[
          [
            "render.rs",
            "2 lines",
            "Fixed Unicode em-dash in Mission Control tab labels",
          ],
          [
            "ui_render.rs",
            "4 lines",
            "Relocated orphaned comment above active_symbol declaration",
          ],
          [
            "system_tools.rs",
            "+21 net",
            "Extracted fetch_panel_data_value() as shared implementation",
          ],
          [
            "thread.rs",
            "−174 lines",
            "Replaced duplicated panel logic with call to shared function",
          ],
        ]}
      />

      <Divider />

      <H2>Verification</H2>
      <Grid columns={3} gap={16}>
        <Stat value="0" label="Warnings" tone="success" />
        <Stat value="895" label="Tests Passed" tone="success" />
        <Stat value="0" label="Tests Failed" tone="success" />
      </Grid>

      <Table
        headers={["Check", "Result", "Details"]}
        rows={[
          ["cargo check", "PASS", "Compiles with no errors or warnings"],
          [
            "cargo test",
            "PASS",
            "895 passed, 0 failed, 3 ignored (including both fetch_panel_data tests)",
          ],
          [
            "Net delta",
            "−146 lines",
            "39 insertions, 185 deletions across 4 files",
          ],
        ]}
        rowTone={["success", "success", undefined]}
      />

      <Divider />

      <H2>Execution Timeline</H2>
      <Timeline
        events={[
          {
            label: "Code review across 43 changed files (5 commits)",
            state: "done",
          },
          {
            label:
              "Identified 4 issues: Unicode, comment, DRY, build confusion",
            state: "done",
          },
          {
            label: "Fixed corrupted Unicode em-dash in render.rs",
            state: "done",
          },
          {
            label: "Relocated orphaned comment in ui_render.rs",
            state: "done",
          },
          {
            label:
              "Extracted shared fetch_panel_data_value() in system_tools.rs",
            state: "done",
          },
          {
            label:
              "Removed run_build from FetchPanelData in thread.rs (−174 lines)",
            state: "done",
          },
          {
            label:
              "Verified: cargo check (0 warnings), cargo test (895 pass)",
            state: "done",
          },
          {
            label: "Committed 3b0d557 and pushed to master",
            state: "done",
          },
        ]}
      />

      <Divider />

      <Text tone="secondary" size="small">
        Generated for Velocity IDE GUI code review — August 2026
      </Text>
    </Stack>
  );
}
