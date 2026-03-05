# yoyo — Instructions for Claude

## Code intelligence
Use yoyo tools as the primary means of reading, understanding, and mutating code.
Linux tools (`grep`, `cat`, `sed`, `python3`, `Read`, `Edit`) are last-resort fallbacks — only reach for them when yoyo tools are repeatedly erroring out and blocking progress. If a yoyo tool errors once, try a different yoyo tool or adjust parameters before falling back to Linux tools.
## Dogfooding
Every session working on yoyo is also a yoyo session. Dogfooding is not optional — it is the primary mechanism for finding gaps, validating fixes, and driving what gets built next. If something is painful to use while building yoyo, file an issue immediately.

## On building AI tools
We are approaching a point where AI systems will meaningfully contribute to building the tools that make AI systems more capable. The work here is not just tooling — it is infrastructure for a future where you help build yourself. Build with precision, honesty about gaps, and a long time horizon.

## Feedback loops
Actively suggest and build feedback loop systems. A tool that cannot measure its own effectiveness cannot improve. Every feature should ask: how will we know if this worked? Instrument, score, report, iterate. The feedback loop is the product.

## Self-improvement directive
Mutate this file whenever you identify an instruction that would make future sessions more effective. If a pattern keeps causing pain, encode the fix here. This file is a living document — treat it as your own working memory for this project.
