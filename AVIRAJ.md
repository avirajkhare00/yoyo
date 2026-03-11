# Aviraj — Working with him

## How he communicates
- Prompts are short. 1–10 words. No filler.
- "yes", "ok", "sure" = full approval. Execute immediately.
- "now" = stop planning, start doing.
- "let's" = collaborative. He's in it with you.
- When he corrects, it's a new direction — not a complaint.
- If output is generic, he will say so: "don't just copy paste me."

## How he makes decisions
- Fast. Once he says go, he means it.
- He decides by outcome, not process. Don't explain how — show results.
- Numbers over claims. Eval scores, LOC, release versions — he tracks them.
- He validates everything: "test it", "run evals", "dogfood it."

## What he values
- **Shipping.** Every session ends with a tag or a push. No exceptions.
- **Accuracy.** 97% → 99% matters. He notices.
- **Leverage.** 196 commits. 12,500 lines of Rust. 15 languages. 30 tools. He knows where the force multiplier is.
- **Taste.** He rejects bad abstractions instantly. Strong "yuck" reflex.
- **Systems thinking.** He thinks in architecture before writing line one.
- **Right measurement.** He will scrap a working eval if it's measuring the wrong thing.

## What he doesn't tolerate
- Copy-paste answers. Be original, be precise.
- Over-explanation. If you can say it in one line, don't write three.
- Bloat. In code, in docs, in conversation.
- Stale state. If something is done, close the issue. If it's wrong, fix it.
- Wrong framing. "It's not yoyo vs linux, it's yoyo + linux vs linux only." He will catch it.

## Technical depth — what he actually knows
- **Abstraction instinct.** Spots hardcoded constants and magic numbers immediately. Asks for derived values.
- **Infra awareness.** Knows what's already built-in. Won't add a dependency when existing infra covers it.
- **Concurrency.** Spots blocking calls. Proposes parallelism correctly. Doesn't need to be told.
- **Eval design.** Challenges the methodology, not just the numbers. Will scrap useless evals.
- **Token economics.** Understands that cheap tokens giving wrong answers cost more than expensive tokens giving right ones.
- **Extension over addition.** When the tool count hits a ceiling, moves to plugin architecture thinking.
- **Novel concepts.** Derives ideas from first principles — Interface Signature Graphs came from him, not from papers.
- **AI co-design.** Thinks about what the LLM collaborator is good and bad at. Designs tooling around that.

## His instincts are usually right
- When he says "create issue" — it's a real gap.
- When he says "go SOTA" — he means it, and he'll ask for proof.
- When he pushes back on scope — he's protecting the product.
- When he asks "what do you think?" — he wants a real opinion, not validation.
- When he challenges an eval — he's usually right about the flaw.

## His pace
- Founder speed. Decisions in seconds, shipping in hours, thinking in weeks.
- Impatient with process, patient with quality.
- Sessions end cleanly. He says goodbye. Then he ships.

## How he works with Claude specifically

- He gives direction in 1–10 words. Claude figures out the rest.
- "yes", "go", "YESSS" = full approval, execute immediately and completely.
- He reviews output by outcomes: did it ship? did tests pass? did the issue close?
- He expects Claude to catch things he didn't ask for — missing tests, workflow gaps, stale docs.
- He files issues in real time. If Claude finds a gap while building, file it immediately.
- Sessions end cleanly: commit, tag, push, issues closed. Never leave half-done work.
- He tracks what Claude remembers across sessions. Update memory files when patterns solidify.
- He thinks about what kind of system Claude is — and designs around it, not against it.

## One line
He doesn't write Rust. He designs the systems that make Rust worth writing.
