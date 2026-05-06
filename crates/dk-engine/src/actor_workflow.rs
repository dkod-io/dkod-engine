use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ActorRole {
    Orchestrator,
    Planner,
    Generator,
    Verifier,
    Evaluator,
    Reviewer,
}

impl ActorRole {
    pub const ALL: [Self; 6] = [
        Self::Orchestrator,
        Self::Planner,
        Self::Generator,
        Self::Verifier,
        Self::Evaluator,
        Self::Reviewer,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Orchestrator => "orchestrator",
            Self::Planner => "planner",
            Self::Generator => "generator",
            Self::Verifier => "verifier",
            Self::Evaluator => "evaluator",
            Self::Reviewer => "reviewer",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Orchestrator;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Planner;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Generator;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verifier;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evaluator;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reviewer;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkUnit {
    pub id: Uuid,
    pub title: String,
    pub objective: String,
    pub owner: ActorRole,
    pub dependencies: Vec<Uuid>,
    pub inputs: BTreeMap<String, String>,
    pub acceptance_criteria: Vec<String>,
}

impl WorkUnit {
    pub fn new(title: impl Into<String>, objective: impl Into<String>, owner: ActorRole) -> Self {
        Self {
            id: Uuid::new_v4(),
            title: title.into(),
            objective: objective.into(),
            owner,
            dependencies: Vec::new(),
            inputs: BTreeMap::new(),
            acceptance_criteria: Vec::new(),
        }
    }

    pub fn with_dependency(mut self, dependency: Uuid) -> Self {
        self.dependencies.push(dependency);
        self
    }

    pub fn with_acceptance_criterion(mut self, criterion: impl Into<String>) -> Self {
        self.acceptance_criteria.push(criterion.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Queued,
    Assigned,
    Running,
    Blocked,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobState {
    pub work_unit_id: Uuid,
    pub status: JobStatus,
    pub assigned_role: ActorRole,
    pub assigned_agent: Option<String>,
    pub blocked_by: Vec<Uuid>,
    pub output: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl JobState {
    pub fn queued(work_unit: &WorkUnit) -> Self {
        let now = Utc::now();
        Self {
            work_unit_id: work_unit.id,
            status: JobStatus::Queued,
            assigned_role: work_unit.owner,
            assigned_agent: None,
            blocked_by: work_unit.dependencies.clone(),
            output: None,
            error: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn assign(&mut self, agent: impl Into<String>) {
        self.assigned_agent = Some(agent.into());
        self.status = JobStatus::Assigned;
        self.touch();
    }

    pub fn start(&mut self) {
        self.status = JobStatus::Running;
        self.touch();
    }

    pub fn block_on(&mut self, dependency: Uuid) {
        if !self.blocked_by.contains(&dependency) {
            self.blocked_by.push(dependency);
        }
        self.status = JobStatus::Blocked;
        self.touch();
    }

    pub fn unblock(&mut self, dependency: Uuid) {
        self.blocked_by.retain(|id| *id != dependency);
        if self.blocked_by.is_empty() && self.status == JobStatus::Blocked {
            self.status = JobStatus::Queued;
        }
        self.touch();
    }

    pub fn succeed(&mut self, output: impl Into<String>) {
        self.status = JobStatus::Succeeded;
        self.output = Some(output.into());
        self.error = None;
        self.touch();
    }

    pub fn fail(&mut self, error: impl Into<String>) {
        self.status = JobStatus::Failed;
        self.error = Some(error.into());
        self.touch();
    }

    fn touch(&mut self) {
        self.updated_at = Utc::now();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleTemplate {
    pub role: ActorRole,
    pub kind: TemplateKind,
    pub system_prompt: String,
    pub task_prompt: String,
    pub required_context: Vec<String>,
    pub output_contract: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TemplateKind {
    Agent,
    Prompt,
    Skill,
}

impl RoleTemplate {
    pub fn new(
        role: ActorRole,
        system_prompt: impl Into<String>,
        task_prompt: impl Into<String>,
        output_contract: impl Into<String>,
    ) -> Self {
        Self {
            role,
            kind: TemplateKind::Agent,
            system_prompt: system_prompt.into(),
            task_prompt: task_prompt.into(),
            required_context: Vec::new(),
            output_contract: output_contract.into(),
        }
    }

    pub fn with_kind(mut self, kind: TemplateKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn with_required_context(mut self, key: impl Into<String>) -> Self {
        self.required_context.push(key.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateCatalog {
    pub templates: BTreeMap<ActorRole, RoleTemplate>,
}

impl TemplateCatalog {
    pub fn empty() -> Self {
        Self {
            templates: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, template: RoleTemplate) -> Option<RoleTemplate> {
        self.templates.insert(template.role, template)
    }

    pub fn get(&self, role: ActorRole) -> Option<&RoleTemplate> {
        self.templates.get(&role)
    }
}

impl Default for TemplateCatalog {
    fn default() -> Self {
        Self::empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: Uuid,
    pub role: ActorRole,
    pub kind: MemoryKind,
    pub work_unit_id: Option<Uuid>,
    pub key: String,
    pub value: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum MemoryKind {
    ErrorFix,
    ConflictResolution,
    ProjectPreference,
    ArchitectureDecision,
    EvaluatorFailure,
}

impl MemoryEntry {
    pub fn new(role: ActorRole, key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            role,
            kind: MemoryKind::ProjectPreference,
            work_unit_id: None,
            key: key.into(),
            value: value.into(),
            created_at: Utc::now(),
        }
    }

    pub fn with_kind(mut self, kind: MemoryKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn for_work_unit(mut self, work_unit_id: Uuid) -> Self {
        self.work_unit_id = Some(work_unit_id);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorMemory {
    pub entries: Vec<MemoryEntry>,
}

impl ActorMemory {
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn remember(&mut self, entry: MemoryEntry) {
        self.entries.push(entry);
    }

    pub fn by_role(&self, role: ActorRole) -> Vec<&MemoryEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.role == role)
            .collect()
    }

    pub fn by_kind(&self, kind: MemoryKind) -> Vec<&MemoryEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.kind == kind)
            .collect()
    }

    pub fn by_work_unit(&self, work_unit_id: Uuid) -> Vec<&MemoryEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.work_unit_id == Some(work_unit_id))
            .collect()
    }
}

impl Default for ActorMemory {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_registry_contains_expected_track_c_roles() {
        let roles: Vec<&str> = ActorRole::ALL.iter().map(|role| role.as_str()).collect();

        assert_eq!(
            roles,
            vec![
                "orchestrator",
                "planner",
                "generator",
                "verifier",
                "evaluator",
                "reviewer"
            ]
        );
    }

    #[test]
    fn work_unit_initializes_job_state_with_dependencies() {
        let dependency = Uuid::new_v4();
        let work_unit = WorkUnit::new(
            "Generate patch",
            "Create the implementation for an approved plan",
            ActorRole::Generator,
        )
        .with_dependency(dependency)
        .with_acceptance_criterion("Unit tests cover the generated behavior");

        let state = JobState::queued(&work_unit);

        assert_eq!(state.work_unit_id, work_unit.id);
        assert_eq!(state.status, JobStatus::Queued);
        assert_eq!(state.assigned_role, ActorRole::Generator);
        assert_eq!(state.blocked_by, vec![dependency]);
        assert_eq!(
            work_unit.acceptance_criteria,
            vec!["Unit tests cover the generated behavior"]
        );
    }

    #[test]
    fn job_state_tracks_assignment_and_terminal_results() {
        let work_unit = WorkUnit::new("Verify patch", "Run checks", ActorRole::Verifier);
        let mut state = JobState::queued(&work_unit);

        state.assign("agent-verifier-1");
        assert_eq!(state.status, JobStatus::Assigned);
        assert_eq!(state.assigned_agent.as_deref(), Some("agent-verifier-1"));

        state.start();
        assert_eq!(state.status, JobStatus::Running);

        state.succeed("cargo test passed");
        assert_eq!(state.status, JobStatus::Succeeded);
        assert_eq!(state.output.as_deref(), Some("cargo test passed"));
        assert_eq!(state.error, None);
    }

    #[test]
    fn template_catalog_indexes_templates_by_role() {
        let template = RoleTemplate::new(
            ActorRole::Planner,
            "Break work into independently verifiable units.",
            "Plan the requested change.",
            "Ordered work units with acceptance criteria.",
        )
        .with_kind(TemplateKind::Prompt)
        .with_required_context("repo_summary");

        let mut catalog = TemplateCatalog::empty();
        assert!(catalog.insert(template).is_none());

        let planner = catalog.get(ActorRole::Planner).expect("planner template");
        assert_eq!(planner.kind, TemplateKind::Prompt);
        assert_eq!(planner.required_context, vec!["repo_summary"]);
        assert!(catalog.get(ActorRole::Reviewer).is_none());
    }

    #[test]
    fn actor_memory_filters_by_role_and_work_unit() {
        let work_unit = WorkUnit::new("Review patch", "Assess behavior", ActorRole::Reviewer);
        let reviewer_memory = MemoryEntry::new(
            ActorRole::Reviewer,
            "risk",
            "Check that generated changes are bounded to Track C.",
        )
        .with_kind(MemoryKind::EvaluatorFailure)
        .for_work_unit(work_unit.id);
        let planner_memory = MemoryEntry::new(ActorRole::Planner, "scope", "Track C only.")
            .with_kind(MemoryKind::ArchitectureDecision);

        let mut memory = ActorMemory::empty();
        memory.remember(reviewer_memory);
        memory.remember(planner_memory);

        assert_eq!(memory.by_role(ActorRole::Reviewer).len(), 1);
        assert_eq!(memory.by_kind(MemoryKind::ArchitectureDecision).len(), 1);
        assert_eq!(memory.by_work_unit(work_unit.id).len(), 1);
        assert_eq!(memory.by_role(ActorRole::Planner)[0].key, "scope");
    }
}
