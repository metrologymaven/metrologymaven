@AGENTS.md

Claude-specific notes:
- When given a target node, follow `prompts/expand-node.md` exactly. Expand only that node.
- Run `python tools/scope.py check` before you finish. Fix errors only in files you touched.
- Funding alignment: read `decisions/D-036-funding-alignment.md` before expanding ECO, TRUST, QUAL, OUTR, KRN, GDT, UNC or AGENT nodes. Founder business and funding context lives outside this repo in `../docs/funding-strategy.md`. Read it for context, but never copy its contents into this repository.
