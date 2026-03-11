# Contributing to yoyo

Thanks for your interest in contributing! yoyo is a code intelligence MCP server built for AI agents, and outside contributions are how we cover more agents, languages, and use cases.

## Quick Start

1. **Fork** the repository
2. **Create a branch** for your changes: `git checkout -b your-feature-name`
3. **Make your changes** following the patterns below
4. **Open a PR** against the `main` branch
5. **Expect a response within 48 hours** — we don't leave PRs stale

## What Makes a Good Contribution

### Adding Agent Support

If you're adding setup instructions for a new agent, follow the existing pattern in README.md:

```markdown
### Agent Name

```bash
# Installation command
# Configuration step
```

See existing examples for Claude Code, Codex CLI, Gemini CLI, and OpenCode.
```

### Code Changes

- Keep PRs small and focused — one topic per PR
- Follow Rust naming conventions and existing code style
- Add tests if you're adding new functionality
- Run `cargo test` before submitting

### Documentation

- Fix typos and grammar errors — these count as contributions
- Improve clarity in existing docs
- Add examples for complex workflows

## PR Checklist

Before submitting, check:

- [ ] Does it follow the existing style/patterns?
- [ ] Did you test the change (command, code, or docs)?
- [ ] Is the PR focused on one topic?
- [ ] Did you branch from latest `main`?

## What Happens After You Submit

1. **We review the diff** — does it work? does it follow style?
2. **We check for overlap** with open PRs — if there's conflict, we'll guide you
3. **We squash-merge** to keep history clean — you'll get credit via `Co-authored-by:`
4. **We thank you and close** — always with a comment explaining the outcome

## Questions?

- Open an issue for discussion before major changes
- Check existing issues for `good first issue` labels
- Look at merged PRs for examples of what we accept

## License

By contributing, you agree that your contributions will be licensed under the Apache 2.0 License.