use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;

// --- TOML deserialization types ---

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowFile {
    pub pipeline: PipelineConfig,
    #[serde(default)]
    pub stage: Vec<StageConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineConfig {
    pub name: String,
    #[serde(default = "default_timeout")]
    pub timeout: String,
}

fn default_timeout() -> String {
    "10m".to_string()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageConfig {
    pub name: String,
    #[serde(default)]
    pub parallel: bool,
    #[serde(default)]
    pub step: Vec<StepConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepConfig {
    pub name: String,
    #[serde(default)]
    pub run: Option<String>,
    #[serde(default, rename = "type")]
    pub step_type: Option<String>,
    #[serde(default)]
    pub timeout: Option<String>,
    #[serde(default)]
    pub changeset_aware: bool,
    #[serde(default = "default_required")]
    pub required: bool,
    #[serde(default)]
    pub check: Vec<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    /// LLM judge criteria (only used when `step_type == "llm-judge"`).
    #[serde(default)]
    pub criteria: Vec<String>,
    /// LLM judge maximum iteration count.
    #[serde(default)]
    pub max_iterations: Option<u32>,
}

fn default_required() -> bool {
    true
}

// --- YAML deserialization types ---

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct YamlWorkflowFile {
    pub pipeline: YamlPipelineConfig,
    #[serde(default)]
    pub stages: Vec<YamlStageConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct YamlPipelineConfig {
    pub name: String,
    #[serde(default = "default_timeout")]
    pub timeout: String,
    #[serde(default)]
    pub allowed_commands: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct YamlStageConfig {
    pub name: String,
    #[serde(default)]
    pub parallel: bool,
    #[serde(default)]
    pub steps: Vec<YamlStepConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct YamlStepConfig {
    pub name: String,
    #[serde(default)]
    pub run: Option<String>,
    #[serde(default, rename = "type")]
    pub step_type: Option<String>,
    #[serde(default)]
    pub timeout: Option<String>,
    #[serde(default)]
    pub changeset_aware: bool,
    #[serde(default = "default_required")]
    pub required: bool,
    #[serde(default)]
    pub check: Vec<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    /// LLM judge criteria (only used when `step_type == "llm-judge"`).
    #[serde(default)]
    pub criteria: Vec<String>,
    /// LLM judge maximum iteration count.
    #[serde(default)]
    pub max_iterations: Option<u32>,
}

// --- Resolved types (post-parsing) ---

#[derive(Debug, Clone)]
pub struct Workflow {
    pub name: String,
    pub timeout: Duration,
    pub stages: Vec<Stage>,
    /// Per-repo command allowlist. Empty means "use default allowlist".
    pub allowed_commands: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Stage {
    pub name: String,
    pub parallel: bool,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone)]
pub struct Step {
    pub name: String,
    pub step_type: StepType,
    pub timeout: Duration,
    pub required: bool,
    pub changeset_aware: bool,
    /// Optional subdirectory to run this step in, relative to the repo root.
    pub work_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub enum StepType {
    Command {
        run: String,
    },
    Semantic {
        checks: Vec<String>,
    },
    AgentReview {
        prompt: String,
    },
    HumanApprove,
    /// LLM-as-judge approval loop. Each iteration re-runs the judge over
    /// the diff + criteria + the previous iteration's reasoning until the
    /// judge returns a terminal verdict or `max_iterations` is reached.
    LlmJudge {
        /// Human-readable criteria the judge must evaluate (e.g. "no
        /// panics in hot paths", "test coverage not reduced"). Rendered
        /// into the judge prompt as a checklist.
        criteria: Vec<String>,
        /// Maximum judge iterations before falling back to `reject`. Each
        /// iteration makes one LLM call. Default: 3.
        max_iterations: u32,
    },
    /// PyTorch test-infra verification step — queries the HUD for
    /// relevant shards and known-flaky tests. Opt-in via `type:
    /// pytorch-ci` in the workflow YAML or `DKOD_PYTORCH_CI=1`.
    PytorchCi,
}
