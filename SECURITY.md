# Security policy

## Reporting a vulnerability

Please report security problems privately, not in a public issue: use GitHub's private vulnerability reporting at
<https://github.com/bullterrier-studio/bullterrier-audio/security/advisories/new> ("Report a vulnerability" on the repository's
Security tab). Say what is affected, how to reproduce it (a small file helps), and what an attacker could do with it.

You can expect an acknowledgement within a week. Once a fix is released, the advisory is published with credit to you,
unless you prefer otherwise.

## Supported versions

bullterrier-audio is before 1.0: only the latest release receives security fixes.

| Version | Supported |
| --- | --- |
| latest 0.x release | yes |
| older releases | no |

## What bullterrier-audio does to stay safe

- **Audited code and dependencies.** ruff's security rules (flake8-bandit) are part of every `poe check`, and
  `uv run poe audit` checks every release pinned in `uv.lock` with pip-audit. No known advisory is open; one that has no
  fixed release yet would be listed here by its id.
- **Models are downloaded only when an analysis that needs them runs**, never at import; the heavy ones (stem separation,
  drum and note transcription) are optional and run in isolated environments.
- **Plugins** are Python packages: installing one runs its code, as with any package.
