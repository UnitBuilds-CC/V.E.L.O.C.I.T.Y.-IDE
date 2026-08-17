import {
  Stack,
  Grid,
  Stat,
  Table,
  Divider,
  Text,
  H1,
  H2,
  Tag,
  Callout,
  Row,
  Code,
  useHostTheme,
} from "qoder/canvas";

export default function GuiReviewFixesReport() {
  const { tokens } = useHostTheme();

  return (
    <Stack gap={20} style={{ padding: 24 }}>
      {/* Header */}
      <Stack gap={6}>
        <Row gap={10} align="center">
          <H1>GUI Code Review — Fixes Complete</H1>
          <Tag tone="success">All 4 issues resolved</Tag>
        </Row>
        <Text tone="secondary" size="small">
          Commit <Code>3b0d557</Code> pushed to <Code>master</Code> · Velocity IDE
          (Kimi-Code) · August 2026
        </Text>
      </Stack>

      <Divider />

      {/* Summary Stats */}
      <Grid columns={4} gap={12}>
        <Stat value="43" label="Files reviewed" />
        <Stat value="4" label="Issues fixed" tone="success" />
        <Stat value="895" label="Tests passing" tone="success" />
        <Stat value="−146" label="Net lines removed" />
      </Grid>

      <Divider />

      {/* Issues Fixed */}
      <H2>Issues Fixed</H2>
      <Table
        headers={["#", "Issue", "Severity", "File", "Resolution"]}
        rows={[
          [
            "1",
            "Corrupted Unicode in Mission Control tab labels",
            "Medium",
            "render.rs",
            "Replaced ? with em-dash (U+2014)",
          ],
          [
            "2",
            "Orphaned comment displaced during toolbar refactor",
            "Low",
            "ui_render.rs",
            "Moved comment back above active_symbol declaration",
          ],
          [
            "3",
            "Duplicated panel-data logic in two modules",
            "Low",
            "system_tools.rs / thread.rs",
            "Extracted shared fetch_panel_data_value()",
          ],
          [
            "4",
            "run_build inside FetchPanelData could signal AgentFinished",
            "Low",
            "thread.rs",
            "Removed — already handled by RunLocalBuild",
          ],
        ]}
        columnRoles={["numeric", undefined, "badge", "code", undefined]}
        rowTone={[
          [undefined, undefined, "warning", undefined, undefined],
          [undefined, undefined, undefined, undefined, undefined],
          [undefined, undefined, undefined, undefined, undefined],
          [undefined, undefined, undefined, undefined, undefined],
        ]}
      />

      <Divider />

      {/* Changed Files Detail */}
      <H2>Changed Files</H2>
      <Table
        headers={["File", "Change", "Description"]}
        rows={[
          [
            "editor/app/render.rs",
            "2 lines",
            "Fixed Unicode em-dash in Work and Activity tab labels",
          ],
          [
            "editor/app/velocity_app/ui_render.rs",
            "4 lines",
            "Relocated orphaned comment to correct position",
          ],
          [
            "registry/system_tools.rs",
            "+21 net",
            "Extracted pub fn fetch_panel_data_value() as shared implementation",
          ],
          [
            "agent/executor/thread.rs",
            "−174",
            "Replaced duplicated panel logic with call to shared function",
          ],
        ]}
        columnRoles={["code", "numeric", undefined]}
      />

      <Divider />

      {/* Verification */}
      <H2>Verification</H2>
      <Grid columns={2} gap={16}>
        <Stack gap={8}>
          <Callout tone="success" title="cargo check">
            Compiles with no errors or warnings
          </Callout>
          <Callout tone="success" title="cargo test">
            895 passed · 0 failed · 3 ignored
          </Callout>
        </Stack>
        <Stack gap={8}>
          <Callout tone="info" title="fetch_panel_data tests">
            Both tests pass: lists_files_and_is_advertised and
            rejects_unknown_panel
          </Callout>
          <Callout tone="info" title="Diff summary">
            39 insertions · 185 deletions across 4 files
          </Callout>
        </Stack>
      </Grid>

      <Divider />

      {/* Outcome */}
      <H2>Outcome</H2>
      <Stack gap={8}>
        <Text>
          All 4 review issues resolved in a single commit. The codebase is now:
        </Text>
        <Stack gap={4}>
          <Row gap={8} align="center">
            <Tag tone="success">Clean</Tag>
            <Text size="small">
              Net −146 lines; removed dead code and duplication
            </Text>
          </Row>
          <Row gap={8} align="center">
            <Tag tone="success">DRY</Tag>
            <Text size="small">
              Single <Code>fetch_panel_data_value()</Code> implementation shared
              by MCP tools and agent thread
            </Text>
          </Row>
          <Row gap={8} align="center">
            <Tag tone="success">Correct</Tag>
            <Text size="small">
              Proper Unicode rendering, correct comment placement, no
              build/action handler confusion
            </Text>
          </Row>
        </Stack>
      </Stack>

      <Divider />

      <Text tone="secondary" size="small">
        Generated from Quest goal completion · GUI Code Review Fixes
      </Text>
    </Stack>
  );
}
