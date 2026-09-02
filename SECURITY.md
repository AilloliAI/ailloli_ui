# Security Policy

## Supported versions

Ailloli UI provides security fixes on a best-effort basis for the latest beta
line only. Publishing a newer beta ends support for every older beta; fixes are
not backported across pre-release lines.

| Release | Supported |
| --- | --- |
| Latest published beta | Yes |
| Earlier betas | No |

Pre-release interfaces can change. This support statement is not a service
level agreement and does not promise a response or resolution time.

An unpublished release candidate does not change this table: the current
published beta remains supported until the newer beta's exact tag and artifacts
are published. Candidate source revisions may receive fixes during preparation,
but become supported releases only upon publication. A fix that changes source
is issued under a new version; published tags and crates are never replaced in
place.

## Report a vulnerability privately

Do not open a public issue, pull request, discussion, or social-media post for
a suspected vulnerability. Use a
[private GitHub Security Advisory](https://github.com/AilloliAI/ailloli_ui/security/advisories/new)
instead. Include, when possible:

- the affected revision and feature set;
- the operating system and Rust version;
- a minimal reproduction or proof of concept;
- the expected impact and any known mitigations;
- whether the report or your identity may be acknowledged publicly.

Avoid including unrelated personal data, credentials, production data, or
third-party secrets. If GitHub private vulnerability reporting is not yet
available, wait for that channel to be enabled rather than publishing the
details in an issue.

## Handling and disclosure

Maintainers will acknowledge, reproduce, assess, and remediate reports as
capacity permits. Coordinated disclosure timing is agreed with the reporter
after a fix and validation path are understood. A report may be closed when it
cannot be reproduced, is outside the repository's scope, or does not create a
security boundary violation.

Sponsorship never buys faster handling, advance access, disclosure control, or
priority over another security report. Commercial support arrangements, if
offered in the future, are separate contracts and do not weaken this public
security process.
