#!/usr/bin/env python3
"""Dekode AI Agent — uses Claude to search and modify code via the Dekode Agent Protocol.

Usage:
    python claude_agent.py --server localhost:50051 --token SECRET --codebase my-repo "add error handling to parse_config"

Requires:
    pip install dekode[anthropic]
    export ANTHROPIC_API_KEY=sk-ant-...
"""

from __future__ import annotations

import argparse
import sys

from anthropic import Anthropic

from dekode import DekodeClient, dekode_tools, dispatch_tool


def main() -> None:
    parser = argparse.ArgumentParser(description="Dekode AI Agent powered by Claude")
    parser.add_argument("--server", default="localhost:50051", help="Dekode server address")
    parser.add_argument("--token", required=True, help="Auth token for the Dekode server")
    parser.add_argument("--codebase", required=True, help="Codebase name to connect to")
    parser.add_argument("--model", default="claude-sonnet-4-5-20250929", help="Claude model to use")
    parser.add_argument("task", help="Task for the agent (e.g. 'add error handling to parse_config')")
    args = parser.parse_args()

    # Connect to Dekode
    client = DekodeClient(args.server, auth_token=args.token, agent_id="claude-agent")
    session = client.connect(codebase=args.codebase, intent=args.task)
    print(f"Connected to '{args.codebase}' (version: {session.codebase_version})")
    print(f"  Languages: {', '.join(session.summary.languages)}")
    print(f"  Symbols: {session.summary.total_symbols}, Files: {session.summary.total_files}")
    print()

    # Set up Claude
    anthropic = Anthropic()
    tools = dekode_tools(session)
    messages: list[dict] = [{"role": "user", "content": args.task}]

    system_prompt = (
        "You are a coding agent connected to a codebase via the Dekode platform. "
        "Use search_codebase to find relevant code, then submit_changes to make modifications. "
        "Always search first to understand the current code before making changes. "
        "Explain your reasoning as you work."
    )

    # Agentic loop
    print(f"Task: {args.task}")
    print("-" * 60)

    while True:
        response = anthropic.messages.create(
            model=args.model,
            max_tokens=4096,
            system=system_prompt,
            tools=tools,
            messages=messages,
        )

        # Process response blocks
        assistant_content = []
        for block in response.content:
            assistant_content.append(block)
            if block.type == "text":
                print(f"\nClaude: {block.text}")
            elif block.type == "tool_use":
                print(f"\n[Tool: {block.name}({block.input})]")

        messages.append({"role": "assistant", "content": assistant_content})

        # If there were tool uses, dispatch and add results
        tool_uses = [b for b in response.content if b.type == "tool_use"]
        if tool_uses:
            tool_results = []
            for block in tool_uses:
                result = dispatch_tool(session, block.name, block.input)
                print(f"[Result: {result[:200]}{'...' if len(result) > 200 else ''}]")
                tool_results.append(
                    {
                        "type": "tool_result",
                        "tool_use_id": block.id,
                        "content": result,
                    }
                )
            messages.append({"role": "user", "content": tool_results})
        else:
            break

        if response.stop_reason == "end_turn":
            break

    print("\n" + "-" * 60)
    print("Done.")
    session.close()


if __name__ == "__main__":
    main()
