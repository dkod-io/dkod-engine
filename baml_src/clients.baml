// LLM client bindings. dkod reads secrets via the DK_LLM_* env vars so all
// provider keys stay out of the tree.
//
// Usage from Rust: `baml_client::b().with_client("dkod-default")`
client<llm> DkodDefault {
    provider openai-generic
    retry_policy Exponential
    options {
        base_url env.DK_LLM_BASE_URL
        api_key  env.DK_LLM_API_KEY
        model    env.DK_LLM_MODEL
    }
}

client<llm> DkodJudge {
    // Separate client so the LLM-as-a-judge loop (dk-runner review gate) can
    // point at a stricter / larger model than the default code agent.
    provider openai-generic
    retry_policy Exponential
    options {
        base_url env.DK_LLM_JUDGE_BASE_URL
        api_key  env.DK_LLM_JUDGE_API_KEY
        model    env.DK_LLM_JUDGE_MODEL
    }
}

client<llm> DkodAnthropic {
    provider anthropic
    retry_policy Exponential
    options {
        api_key env.CLAUDE_API_KEY
        model "claude-sonnet-4-5"
    }
}

retry_policy Exponential {
    max_retries 3
    strategy {
        type exponential_backoff
        delay_ms 300
        multiplier 2.0
        max_delay_ms 10000
    }
}
