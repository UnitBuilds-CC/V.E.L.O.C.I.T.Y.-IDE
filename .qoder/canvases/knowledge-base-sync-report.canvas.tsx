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
} from "qoder/canvas";

export default function KnowledgeBaseSyncReport() {
  return (
    <Stack gap={20} style={{ padding: 24 }}>
      {/* Header */}
      <Stack gap={6}>
        <Row gap={10} align="center">
          <H1>.qoder Knowledge Base — Full Sync</H1>
          <Tag tone="success">Complete</Tag>
        </Row>
        <Text tone="secondary" size="small">
          Comprehensive update of all repowiki, skills, specs, and metadata to reflect
          the current codebase state. August 2026.
        </Text>
      </Stack>

      <Divider />

      {/* Metrics */}
      <Grid columns={4} gap={12}>
        <Stat value="12" label="Knowledge Cards" tone="success" />
        <Stat value="10" label="Content Pages" />
        <Stat value="3" label="Skills" />
        <Stat value="527" label="Source Files Tracked" />
      </Grid>

      <Divider />

      {/* What Changed */}
      <H2>Changes Made</H2>
      <Table
        headers={["Area", "Change", "Detail"]}
        rows={[
          [
            "Metadata",
            "File counts corrected",
            "velocity-mcp: 261 → 257 (benchmark removed), total: 522 → 527",
          ],
          [
            "Metadata",
            "New constraints added",
            "NDA-4bit FP4 weight format, dual-path engine routing",
          ],
          [
            "Knowledge Card",
            "Updated: Compiler & NDA Pipeline",
            "Added model inference, dual-path engine, tokenizer, 12 driver files",
          ],
          [
            "Knowledge Card",
            "Updated: MCP Server & IDE Editor",
            "File count 257, benchmark module removed note, 119 egui files",
          ],
          [
            "Knowledge Card",
            "Updated: Sub-1k LOC Rule",
            "LOC violations refreshed: struct_def 1133, ui_render 2058, render 1878",
          ],
          [
            "Knowledge Card",
            "NEW: LLM Inference Harness",
            "Qwen 2.5 Coder 0.5B, NDA-4bit, zero-alloc forward, fused GEMV",
          ],
          [
            "Knowledge Card",
            "NEW: Dual-Path Engine",
            "Text ↔ NDA routing, hidden_state[896] conditioning, lazy loading",
          ],
          [
            "Content Page",
            "Updated: NDA Compiler & JIT Pipeline",
            "Added model inference, dual-path engine, tokenizer sections",
          ],
          [
            "Content Page",
            "Updated: Workspace Architecture",
            "velocity-mcp 257 files, model inference, dual-path, built-in LLM",
          ],
          [
            "Content Page",
            "Updated: Development Guide",
            "File counts, dual-path engine in velocity-ide description",
          ],
          [
            "Content Page",
            "Updated: Getting Started",
            "527 files, NDA-4bit weights, dual-path, built-in LLM in tree",
          ],
          [
            "Skill",
            "NEW: harness-updater",
            "Port OS→IDE upgrades, NDA weight conversion, shader compatibility",
          ],
          [
            "Skill",
            "Updated: rust-code-review",
            "Added LLM Inference Harness high-risk area checklist",
          ],
          [
            "Spec",
            "Updated: qoder_workspace_setup.md",
            "New cards, new skill, updated maintenance notes",
          ],
          ["Index", "Updated: _index.yaml", "527 files, model module, top-level modules"],
        ]}
      />

      <Divider />

      {/* New Knowledge Cards */}
      <H2>New Knowledge Cards</H2>
      <Grid columns={2} gap={12}>
        <Stack gap={4}>
          <Text weight="semibold">LLM Inference Harness with NDA-4bit Weights</Text>
          <Text tone="secondary" size="small">
            Qwen 2.5 Coder 0.5B config, FP4 weight loading, fused GEMV (Q‖K‖V),
            zero-alloc forward_one, in-place lm_head, dual-path engine, BPE tokenizer,
            GPU acceleration via Vulkan compute shaders.
          </Text>
        </Stack>
        <Stack gap={4}>
          <Text weight="semibold">Dual-Path Engine Text and NDA Routing</Text>
          <Text tone="secondary" size="small">
            DualPathEngine routes text → hidden_state[896] → NDA program generation.
            Auto-mode routing, lazy loading, shared weights, separate state.
            NDA-KV cache with Merkle hash chains.
          </Text>
        </Stack>
      </Grid>

      <Divider />

      {/* Verification */}
      <H2>Verification</H2>
      <Table
        headers={["Check", "Result", "Notes"]}
        rows={[
          [
            "File counts match disk",
            "527 total .rs files (excl. target/archive/scratch)",
            "mcp=257, browser=171, ide=77, drone=5, e2e=4",
          ],
          [
            "LOC violations current",
            "4 files over 1k, 1 resolved",
            "system_tools 1519, struct_def 1133, ui_render 2058, render 1878",
          ],
          [
            "All knowledge cards reference real code",
            "Verified against source tree",
            "pipeline_bridge.rs, tokenizer.rs, model/ all confirmed",
          ],
          [
            "Skills reference real files",
            "All paths verified",
            "config.rs, weights.rs, transformer.rs, nda_gemv.rs confirmed",
          ],
        ]}
      />

      <Callout tone="info">
        <Text size="small">
          The knowledge base now fully reflects the codebase after: GUI code review
          (4 issues fixed, commit <Code>3b0d557</Code>), harness upgrade from OS
          (fused GEMV, zero-alloc forward, in-place LM head), benchmark module cleanup,
          and comprehensive codebase audit.
        </Text>
      </Callout>

      <Text tone="secondary" size="small">
        Generated for .qoder knowledge base sync. 12 knowledge cards, 10 content pages,
        3 skills, 1 spec — all current as of August 2026.
      </Text>
    </Stack>
  );
}
