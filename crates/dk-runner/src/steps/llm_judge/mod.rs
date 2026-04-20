//! LLM-as-judge approval gate.
//!
//! A workflow step that replaces (or supplements) the traditional
//! `human-approve` gate with an LLM that judges a changeset against a
//! configured list of criteria. The step runs an *iteration loop*: on
//! each iteration the judge sees the diff, the criteria, and the previous
//! iteration's critique, and returns a structured verdict
//! ([`JudgeVerdict`]). The loop terminates as soon as the judge returns
//! [`JudgeVerdict::Approve`] or [`JudgeVerdict::Reject`], or when the
//! configured `max_iterations` is exhausted (at which point the step is
//! treated as `Fail` with a finding explaining the non-terminating
//! verdict).
//!
//! The iteration-with-reflection pattern is borrowed from "self-refine"
//! style approaches — the judge gets to *re-read* its own previous
//! critique, which empirically reduces flip-flopping and rewards
//! thinking-out-loud behaviour without us having to pay for extended
//! thinking tokens explicitly.
//!
//! # Providers
//!
//! [`JudgeProvider`] is a trait so tests can inject a deterministic
//! scripted judge (see [`ScriptedJudge`]). In production,
//! [`AnthropicJudge`] is constructed from the environment
//! (`ANTHROPIC_API_KEY` / `DKOD_REVIEW_API_KEY`, `DKOD_JUDGE_MODEL`).
//!
//! # Side-effects
//!
//! On a terminal `Approve` verdict the step flips the changeset to
//! `approved` in the same way [`crate::steps::human_approve`] would, so
//! the rest of the pipeline (deep review, merge) can proceed unmodified.
//! On `Reject` the changeset is flipped to `rejected`. This makes
//! `llm-judge` a drop-in replacement for `human-approve` — with the
//! caveat that a misconfigured judge prompt can approve things a human
//! wouldn't, so the recommended deployment is `llm-judge` *plus* a
//! downstream human spot-check.
//!
//! # Observability
//!
//! Each iteration emits a [`JudgeIteration`] entry into the returned
//! [`JudgeTranscript`], which is serialised into the step's stdout as
//! JSON so MCP clients and dashboards can render the reasoning trail.
//! Findings from the final iteration are surfaced as [`Finding`] rows.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{info, warn};
use uuid::Uuid;

use crate::executor::{StepOutput, StepStatus};
use crate::findings::{Finding, Severity};
use dk_engine::repo::Engine;

/// Terminal verdict of a single judge iteration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgeVerdict {
    /// Judge is satisfied — the changeset meets all criteria.
    Approve,
    /// Judge has reservations that may or may not be fixable via another
    /// iteration. The loop continues until the judge returns a terminal
    /// verdict or iterations are exhausted.
    NeedsIteration,
    /// Judge is decisively against merging.
    Reject,
}

/// Parsed output of a single judge invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JudgeDecision {
    pub verdict: JudgeVerdict,
    /// Free-form reasoning that is fed back into the next iteration. Kept
    /// short — we don't want context to balloon across iterations.
    pub reasoning: String,
    /// Per-criterion notes, when the judge chooses to emit them. Empty
    /// when the judge didn't structure its answer that way.
    #[serde(default)]
    pub criterion_notes: Vec<CriterionNote>,
}

/// Judge's note about a single criterion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriterionNote {
    pub criterion: String,
    pub passed: bool,
    pub note: String,
}

/// One entry in the judge's iteration transcript.
#[derive(Debug, Clone, Serialize)]
pub struct JudgeIteration {
    pub iteration: u32,
    pub decision: JudgeDecision,
    pub duration_ms: u64,
}

/// Full transcript of a judge loop, surfaced as JSON in the step stdout
/// so MCP clients can render the reasoning trail.
#[derive(Debug, Clone, Serialize)]
pub struct JudgeTranscript {
    pub iterations: Vec<JudgeIteration>,
    pub final_verdict: JudgeVerdict,
    pub criteria: Vec<String>,
}

/// Minimal request an LLM judge provider receives per iteration.
#[derive(Debug, Clone)]
pub struct JudgeRequest<'a> {
    pub diff: &'a str,
    pub criteria: &'a [String],
    pub iteration: u32,
    pub previous: &'a [JudgeIteration],
}

/// Abstract judge provider so tests can substitute a scripted sequence
/// of [`JudgeDecision`]s without making any network calls.
#[async_trait::async_trait]
pub trait JudgeProvider: Send + Sync {
    async fn judge(&self, req: JudgeRequest<'_>) -> Result<JudgeDecision>;
}

/// Deterministic judge backed by a pre-recorded sequence of decisions.
/// Used by unit tests but exposed outside `#[cfg(test)]` so downstream
/// integration tests can script it too without enabling a feature flag.
pub struct ScriptedJudge {
    decisions: std::sync::Mutex<std::collections::VecDeque<JudgeDecision>>,
}

impl ScriptedJudge {
    pub fn new(decisions: Vec<JudgeDecision>) -> Self {
        Self {
            decisions: std::sync::Mutex::new(decisions.into()),
        }
    }
}

#[async_trait::async_trait]
impl JudgeProvider for ScriptedJudge {
    async fn judge(&self, _req: JudgeRequest<'_>) -> Result<JudgeDecision> {
        self.decisions
            .lock()
            .expect("ScriptedJudge mutex poisoned — the only way this can happen is if a test panicked mid-judge, which we want to surface")
            .pop_front()
            .context("ScriptedJudge ran out of decisions — test bug")
    }
}

/// Production judge backed by Anthropic's messages API. Mirrors the
/// configuration surface of [`crate::steps::agent_review::claude::ClaudeReviewProvider`]
/// so operators have a single mental model for "LLM-in-the-pipeline".
pub struct AnthropicJudge {
    client: reqwest::Client,
    api_key: String,
    model: String,
    max_tokens: usize,
}

impl AnthropicJudge {
    pub fn new(api_key: String, model: Option<String>) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;
        Ok(Self {
            client,
            api_key,
            model: model.unwrap_or_else(|| "claude-opus-4-7".to_string()),
            max_tokens: 4096,
        })
    }

    /// Build an [`AnthropicJudge`] from the environment. Returns `None`
    /// when no API key is configured — callers are expected to fall back
    /// to a scripted / no-op provider in that case.
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("DKOD_JUDGE_API_KEY")
            .or_else(|_| std::env::var("DKOD_REVIEW_API_KEY"))
            .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
            .ok()?;
        let model = std::env::var("DKOD_JUDGE_MODEL").ok();
        Self::new(api_key, model)
            .map_err(|e| {
                tracing::error!("AnthropicJudge failed to initialise: {e:#}");
                e
            })
            .ok()
    }
}

#[async_trait::async_trait]
impl JudgeProvider for AnthropicJudge {
    async fn judge(&self, req: JudgeRequest<'_>) -> Result<JudgeDecision> {
        let prompt = build_judge_prompt(&req);
        let body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "messages": [
                {"role": "user", "content": prompt}
            ],
        });
        let resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
            .context("anthropic judge request failed")?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .context("anthropic judge: reading body failed")?;
        if !status.is_success() {
            anyhow::bail!("anthropic judge returned HTTP {status}: {text}");
        }
        let parsed: serde_json::Value =
            serde_json::from_str(&text).context("anthropic judge: JSON parse failed")?;
        let content = parsed
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.iter().find_map(|b| b.get("text")?.as_str()))
            .unwrap_or("")
            .to_string();
        parse_judge_decision(&content).with_context(|| {
            format!("anthropic judge: could not parse decision from body: {content}")
        })
    }
}

/// Render the judge prompt. Kept as a free function so tests can snapshot
/// it without spinning up an HTTP provider.
pub fn build_judge_prompt(req: &JudgeRequest<'_>) -> String {
    let mut prompt = String::new();
    prompt.push_str(
        "You are an automated judge in a code review pipeline. Your job is \
        to decide whether a changeset should be *approved*, *rejected*, or \
        whether you *need another iteration* to re-examine it. Be strict \
        but fair — never approve code that plausibly breaks production.\n\n",
    );
    prompt.push_str("## Criteria\n");
    if req.criteria.is_empty() {
        prompt.push_str("- (none configured — use general code-quality judgement)\n");
    } else {
        for c in req.criteria {
            prompt.push_str(&format!("- {c}\n"));
        }
    }
    prompt.push_str("\n## Changeset diff\n```diff\n");
    prompt.push_str(req.diff);
    prompt.push_str("\n```\n\n");

    if !req.previous.is_empty() {
        prompt.push_str("## Your previous iterations\n");
        for iter in req.previous {
            prompt.push_str(&format!(
                "### Iteration {} → {:?}\n{}\n\n",
                iter.iteration, iter.decision.verdict, iter.decision.reasoning
            ));
        }
        prompt.push_str(
            "If your previous reasoning is now stale or wrong, say so \
            and correct it. Do not rubber-stamp your own earlier output.\n\n",
        );
    }

    prompt.push_str(&format!(
        "## Instructions\nThis is iteration {}. Respond with a single JSON \
        object on its own line — do not wrap it in prose or markdown — with \
        this shape:\n\
        {{\"verdict\": \"approve\" | \"needs_iteration\" | \"reject\", \
          \"reasoning\": \"short prose\", \
          \"criterion_notes\": [ {{\"criterion\": \"...\", \"passed\": bool, \"note\": \"...\"}} ]}}\n",
        req.iteration
    ));
    prompt
}

/// Parse the judge's decision JSON out of a free-form LLM response.
///
/// The judge is *instructed* to emit JSON-only, but LLMs sometimes wrap
/// the JSON in prose or fences. We scan for the first `{` and last `}`
/// and try to parse that slice — matches the forgiving pattern used in
/// `agent_review::parse`.
pub fn parse_judge_decision(text: &str) -> Result<JudgeDecision> {
    let start = text
        .find('{')
        .context("no '{' found in judge response — not JSON")?;
    let end = text
        .rfind('}')
        .context("no '}' found in judge response — truncated JSON?")?;
    if end <= start {
        anyhow::bail!("judge response braces are mis-ordered");
    }
    let slice = &text[start..=end];
    let decision: JudgeDecision =
        serde_json::from_str(slice).context("judge JSON did not match schema")?;
    Ok(decision)
}

/// Run the judge loop against a provider with a specific diff + criteria.
///
/// This is the pure entry point — all side effects (DB status flips,
/// clickhouse emissions) are performed by
/// [`run_llm_judge_step_with_engine`], which wraps this.
pub async fn run_judge_loop(
    provider: &dyn JudgeProvider,
    diff: &str,
    criteria: &[String],
    max_iterations: u32,
) -> (JudgeTranscript, Vec<Finding>) {
    let mut iterations: Vec<JudgeIteration> = Vec::new();
    let mut final_verdict = JudgeVerdict::NeedsIteration;

    for i in 1..=max_iterations {
        let started = Instant::now();
        let req = JudgeRequest {
            diff,
            criteria,
            iteration: i,
            previous: &iterations,
        };
        let decision = match provider.judge(req).await {
            Ok(d) => d,
            Err(e) => {
                warn!(error = %e, iteration = i, "llm-judge: provider error");
                // Surface as a "reject" so the step fails — a judge we
                // can't reach is indistinguishable from a judge that
                // decided "no" from the pipeline's perspective.
                final_verdict = JudgeVerdict::Reject;
                iterations.push(JudgeIteration {
                    iteration: i,
                    decision: JudgeDecision {
                        verdict: JudgeVerdict::Reject,
                        reasoning: format!("judge provider error: {e}"),
                        criterion_notes: vec![],
                    },
                    duration_ms: started.elapsed().as_millis() as u64,
                });
                break;
            }
        };
        final_verdict = decision.verdict;
        iterations.push(JudgeIteration {
            iteration: i,
            decision,
            duration_ms: started.elapsed().as_millis() as u64,
        });
        if matches!(final_verdict, JudgeVerdict::Approve | JudgeVerdict::Reject) {
            break;
        }
    }

    // If we exhausted iterations without a terminal verdict, we treat it
    // as a reject — an undecided judge should never auto-approve.
    if matches!(final_verdict, JudgeVerdict::NeedsIteration) {
        final_verdict = JudgeVerdict::Reject;
    }

    let findings = findings_from_iterations(&iterations, final_verdict);
    (
        JudgeTranscript {
            iterations,
            final_verdict,
            criteria: criteria.to_vec(),
        },
        findings,
    )
}

fn findings_from_iterations(
    iterations: &[JudgeIteration],
    final_verdict: JudgeVerdict,
) -> Vec<Finding> {
    let Some(last) = iterations.last() else {
        return vec![];
    };
    let severity = match final_verdict {
        JudgeVerdict::Approve => return vec![],
        JudgeVerdict::Reject => Severity::Error,
        JudgeVerdict::NeedsIteration => Severity::Warning,
    };
    let mut findings = vec![Finding {
        severity,
        check_name: "llm-judge".to_string(),
        message: last.decision.reasoning.clone(),
        file_path: None,
        line: None,
        symbol: None,
    }];
    for note in &last.decision.criterion_notes {
        if note.passed {
            continue;
        }
        findings.push(Finding {
            severity: Severity::Warning,
            check_name: format!("llm-judge:{}", note.criterion),
            message: note.note.clone(),
            file_path: None,
            line: None,
            symbol: None,
        });
    }
    findings
}

/// Run the llm-judge step with engine-side side effects (status flips +
/// analytics emission).
///
/// `diff` is the textual diff the judge should evaluate. The caller is
/// responsible for building it — scheduler.rs stitches file contents
/// together the same way it does for `agent_review`.
pub async fn run_llm_judge_step_with_engine(
    provider: &dyn JudgeProvider,
    engine: &Arc<Engine>,
    changeset_id: Uuid,
    diff: &str,
    criteria: &[String],
    max_iterations: u32,
) -> (StepOutput, Vec<Finding>) {
    let start = Instant::now();
    info!(
        changeset_id = %changeset_id,
        max_iterations,
        "llm-judge: starting judge loop"
    );

    // Mark the changeset as awaiting approval so concurrent viewers see
    // the judge is working on it. Only transition from "draft" — if a
    // human or another judge already approved/rejected, leave it alone.
    if let Err(e) = engine
        .changeset_store()
        .update_status_if(changeset_id, "awaiting_approval", &["draft"])
        .await
    {
        // Non-fatal — if the DB is broken the judge itself will likely
        // also fail; we still want to return a useful StepOutput.
        warn!(error = %e, "llm-judge: could not flip to awaiting_approval");
    }

    let (transcript, findings) = run_judge_loop(provider, diff, criteria, max_iterations).await;

    // Side-effect: flip changeset state based on the verdict. We use
    // `update_status_if` so we don't stomp a human override that may
    // have landed while the judge was thinking.
    let status = match transcript.final_verdict {
        JudgeVerdict::Approve => StepStatus::Pass,
        JudgeVerdict::Reject | JudgeVerdict::NeedsIteration => StepStatus::Fail,
    };
    let new_state = match transcript.final_verdict {
        JudgeVerdict::Approve => "approved",
        _ => "rejected",
    };
    if let Err(e) = engine
        .changeset_store()
        .update_status_if(changeset_id, new_state, &["awaiting_approval"])
        .await
    {
        warn!(error = %e, new_state, "llm-judge: status flip failed");
    }

    // Surface the transcript in stdout as JSON so clients can render it.
    let stdout = serde_json::to_string_pretty(&transcript).unwrap_or_else(|_| String::from("{}"));

    // Emit to analytics — one review_results row with provider "llm-judge"
    // so dashboards can distinguish judge verdicts from deep-review ones.
    emit_judge_analytics(changeset_id, &transcript, start.elapsed());

    (
        StepOutput {
            status,
            stdout,
            stderr: String::new(),
            duration: start.elapsed(),
        },
        findings,
    )
}

fn emit_judge_analytics(changeset_id: Uuid, transcript: &JudgeTranscript, elapsed: Duration) {
    let verdict = match transcript.final_verdict {
        JudgeVerdict::Approve => "approve",
        JudgeVerdict::Reject => "reject",
        JudgeVerdict::NeedsIteration => "needs_iteration",
    };
    let findings_count = transcript
        .iterations
        .last()
        .map(|i| {
            i.decision
                .criterion_notes
                .iter()
                .filter(|n| !n.passed)
                .count() as u32
        })
        .unwrap_or(0);
    dk_analytics::global::emit(dk_analytics::AnalyticsEvent::Review(
        dk_analytics::ReviewResult {
            review_id: Uuid::new_v4(),
            changeset_id,
            provider: "llm-judge".to_string(),
            model: std::env::var("DKOD_JUDGE_MODEL").unwrap_or_default(),
            score: None,
            findings_count,
            verdict: verdict.to_string(),
            duration_ms: elapsed.as_millis() as u64,
            created_at: chrono::Utc::now(),
        },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_judge_decision_strict_json() {
        let text = r#"{"verdict":"approve","reasoning":"LGTM","criterion_notes":[]}"#;
        let d = parse_judge_decision(text).unwrap();
        assert_eq!(d.verdict, JudgeVerdict::Approve);
    }

    #[test]
    fn parse_judge_decision_with_prose() {
        let text = r#"Here is my verdict:

        {"verdict":"reject","reasoning":"panics","criterion_notes":[]}

        That's all."#;
        let d = parse_judge_decision(text).unwrap();
        assert_eq!(d.verdict, JudgeVerdict::Reject);
        assert_eq!(d.reasoning, "panics");
    }

    #[test]
    fn judge_prompt_includes_criteria_and_diff() {
        let diff = "--- a\n+++ b\n@@ -1 +1 @@\n-foo\n+bar\n";
        let criteria = vec!["no panics".to_string(), "tests added".to_string()];
        let req = JudgeRequest {
            diff,
            criteria: &criteria,
            iteration: 1,
            previous: &[],
        };
        let prompt = build_judge_prompt(&req);
        assert!(prompt.contains("no panics"));
        assert!(prompt.contains("tests added"));
        assert!(prompt.contains("+bar"));
        assert!(prompt.contains("iteration 1"));
    }

    #[tokio::test]
    async fn judge_loop_terminates_on_approve() {
        let judge = ScriptedJudge::new(vec![JudgeDecision {
            verdict: JudgeVerdict::Approve,
            reasoning: "fine".into(),
            criterion_notes: vec![],
        }]);
        let (t, f) = run_judge_loop(&judge, "diff", &[], 3).await;
        assert_eq!(t.final_verdict, JudgeVerdict::Approve);
        assert_eq!(t.iterations.len(), 1);
        assert!(f.is_empty());
    }

    #[tokio::test]
    async fn judge_loop_iterates_then_rejects() {
        let judge = ScriptedJudge::new(vec![
            JudgeDecision {
                verdict: JudgeVerdict::NeedsIteration,
                reasoning: "unsure".into(),
                criterion_notes: vec![],
            },
            JudgeDecision {
                verdict: JudgeVerdict::NeedsIteration,
                reasoning: "still unsure".into(),
                criterion_notes: vec![],
            },
            JudgeDecision {
                verdict: JudgeVerdict::Reject,
                reasoning: "no".into(),
                criterion_notes: vec![CriterionNote {
                    criterion: "tests".into(),
                    passed: false,
                    note: "missing".into(),
                }],
            },
        ]);
        let (t, f) = run_judge_loop(&judge, "diff", &["tests".to_string()], 5).await;
        assert_eq!(t.final_verdict, JudgeVerdict::Reject);
        assert_eq!(t.iterations.len(), 3);
        assert!(f.iter().any(|x| x.check_name == "llm-judge:tests"));
    }

    #[tokio::test]
    async fn judge_loop_exhausts_and_falls_back_to_reject() {
        let judge = ScriptedJudge::new(vec![
            JudgeDecision {
                verdict: JudgeVerdict::NeedsIteration,
                reasoning: "1".into(),
                criterion_notes: vec![],
            },
            JudgeDecision {
                verdict: JudgeVerdict::NeedsIteration,
                reasoning: "2".into(),
                criterion_notes: vec![],
            },
        ]);
        let (t, _f) = run_judge_loop(&judge, "diff", &[], 2).await;
        assert_eq!(t.final_verdict, JudgeVerdict::Reject);
        assert_eq!(t.iterations.len(), 2);
    }
}
