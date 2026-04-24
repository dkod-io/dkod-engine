// Agent / workflow graph primitives for dkod.
//
// Each of these BAML functions compiles to a typed Rust function in
// `baml_client/`. dk-runner composes them into a graph at runtime — the
// `WorkflowPlan` returned by `PlanWorkflow` is the edge list that drives
// the runner's state machine.

// ---- Shared types ----------------------------------------------------------

// Agent roles use symbol tuning (@alias "kN") so the planner ranks steps by
// the attached @description text, not by any prior association it has with
// the word "Reviewer" / "Judge" / etc.
// See: https://docs.boundaryml.com/examples/prompt-engineering/symbol-tuning
enum AgentRole {
    Planner  @alias("k1")
             @description("Decomposes the intent into a workflow DAG of small, independently-verifiable steps.")
    Coder    @alias("k2")
             @description("Writes or edits code inside an isolated session overlay; never merges to the base branch directly.")
    Reviewer @alias("k3")
             @description("Reads a changeset diff and produces concrete review notes with blockers, no merge authority.")
    Judge    @alias("k4")
             @description("Scores the changeset + reviewer pair strictly; the only role that can emit a terminal verdict.")
    Verifier @alias("k5")
             @description("Runs lint, typecheck, and tests on the overlay and reports pass/fail with signal names.")
}

class WorkflowStep {
    id          string                @description("Stable step id, referenced by edges.")
    role        AgentRole
    description string                @description("What this step should accomplish.")
    tools       string[]              @description("MCP tool names this step is allowed to call.")
    inputs      string[]              @description("Step ids whose output is consumed here.")
}

class WorkflowEdge {
    from string
    to   string
    // Optional predicate. Empty string means unconditional.
    when string
}

class WorkflowPlan {
    steps WorkflowStep[]
    edges WorkflowEdge[]
    entry string                      @description("Step id to execute first.")
}

// Symbol-tuned decision: previously a `bool approved` field which biases the
// LLM toward the literal word "approved". The three-way outcome carries no
// semantic label the model can anchor to — the @description does all the work.
enum VerdictCode {
    Approve        @alias("k1")
                   @description("No unresolved blockers; the changeset may proceed to merge.")
    RequestChanges @alias("k2")
                   @description("Blockers exist but are addressable by the coder in another iteration.")
    Reject         @alias("k3")
                   @description("Fundamental issue (scope, correctness, safety) — do not iterate on this changeset.")
}

class ReviewVerdict {
    verdict   VerdictCode
    score     int                     @description("0..100, higher is better.")
    rationale string
    blockers  string[]                @description("Concrete issues that must be fixed before approval.")
}

// ---- Functions -------------------------------------------------------------

/// Plan an end-to-end workflow for an agent session given a free-form intent.
/// dk-runner consumes the returned plan and walks the graph, invoking each
/// step via the matching BAML function / MCP tool.
function PlanWorkflow(intent: string, repo_context: string) -> WorkflowPlan {
    client DkodDefault
    prompt #"
        You are the orchestrator for dkod, a code-intelligence engine.
        Produce a minimal workflow plan (DAG) that accomplishes the intent
        using only the agent roles available. Keep steps small and
        independently verifiable. Every non-entry step must list at least
        one input.

        Intent:
        {{ intent }}

        Repo context:
        {{ repo_context }}

        {{ ctx.output_format }}
    "#
}

/// LLM-as-judge gate. Given a changeset summary and a reviewer's notes,
/// decide whether to approve or send back for another coder iteration.
function JudgeChangeset(summary: string, review_notes: string) -> ReviewVerdict {
    client DkodJudge
    prompt #"
        You are the final judge for a dkod changeset. Approve only when the
        reviewer's notes contain no unresolved blocking issues and the
        changeset summary matches the stated intent. Score strictly.

        Changeset summary:
        {{ summary }}

        Reviewer notes:
        {{ review_notes }}

        {{ ctx.output_format }}
    "#
}

/// Deep-review pass used by dk-runner's review gate (lib/agents nodes wire
/// through here). Returns a structured verdict + concrete blockers so the
/// UI can render actionable comments.
function DeepReview(diff: string, context: string) -> ReviewVerdict {
    client DkodAnthropic
    prompt #"
        You are a senior reviewer on the dkod team. Focus on correctness,
        safety, and consistency with the surrounding code. Be concrete.

        Diff:
        ```
        {{ diff }}
        ```

        Context:
        {{ context }}

        {{ ctx.output_format }}
    "#
}

// ---- Tests -----------------------------------------------------------------

test plan_simple_refactor {
    functions [PlanWorkflow]
    args {
        intent "Split `handle_request` into request parsing + business logic."
        repo_context "crates/dk-server/src/grpc.rs ~800 LoC, tonic handler style."
    }
}

test judge_rejects_blockers {
    functions [JudgeChangeset]
    args {
        summary "Adds retry with exponential backoff to SubmitChangeset."
        review_notes "Reviewer: unbounded retry loop can block the executor for minutes. Also no test coverage. Blocking."
    }
}
