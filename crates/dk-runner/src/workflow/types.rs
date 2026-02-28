use serde::Deserialize;
use std::time::Duration;

// --- TOML deserialization types ---

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowFile {
    pub pipeline: PipelineConfig,
    #[serde(default)]
    pub stage: Vec<StageConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PipelineConfig {
    pub name: String,
    #[serde(default = "default_timeout")]
    pub timeout: String,
}

fn default_timeout() -> String {
    "10m".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct StageConfig {
    pub name: String,
    #[serde(default)]
    pub parallel: bool,
    #[serde(default)]
    pub step: Vec<StepConfig>,
}

#[derive(Debug, Clone, Deserialize)]
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
}

fn default_required() -> bool {
    true
}

// --- Resolved types (post-parsing) ---

#[derive(Debug, Clone)]
pub struct Workflow {
    pub name: String,
    pub timeout: Duration,
    pub stages: Vec<Stage>,
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
}

#[derive(Debug, Clone)]
pub enum StepType {
    Command { run: String },
    Semantic { checks: Vec<String> },
    AgentReview { prompt: String },
    HumanApprove,
}
