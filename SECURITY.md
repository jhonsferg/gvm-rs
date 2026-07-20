# Security Policy

## Supported Versions

Only the latest release receives security fixes.

| Version | Supported |
| ------- | --------- |
| 1.x     | ✅        |

## Reporting a Vulnerability

**Please do not open a public GitHub issue for security vulnerabilities.**

Report security issues privately via GitHub's built-in mechanism:

1. Go to the [Security tab](https://github.com/jhonsferg/gvm-rs/security/advisories/new) of this repository.
2. Click **"Report a vulnerability"**.
3. Fill in the details: affected versions, reproduction steps, and potential impact.

You will receive an acknowledgement within **72 hours** and a resolution timeline
within **7 days** for critical issues.

## Scope

- Arbitrary code execution via crafted `.go-version` files or default-packages entries
- Path traversal in archive extraction
- Binary substitution during `gvm install` or `gvm upgrade` (SHA-256 bypass)
- Credential or secret leakage in logs or error messages

## Out of Scope

- Issues in Go toolchains themselves (report to the [Go team](https://go.dev/security))
- Social engineering or phishing
- Vulnerabilities in systems that `gvm` does not control (e.g. go.dev infrastructure)

## Malicious forks / clones

This project is MIT-licensed: forking, modifying, and redistributing it is welcome and expected.
What is **not** covered by that license, and will be treated as a security incident against this
project and its users, is any copy of this repository that:

- Presents itself as this project (same name, README, or commit history) without disclosing that
  it is a modified third-party copy.
- Removes or disables the CI/security workflows present in this repository
  (`ci.yml`, `release.yml`, `security.yml`).
- Distributes executables, archives, or scripts that are not produced by this repository's own
  release pipeline - especially files added directly into source control (`.zip`, `.exe`, `.dll`)
  instead of built by [`release.yml`](.github/workflows/release.yml).
- Instructs users to bypass OS security warnings (SmartScreen, Gatekeeper, antivirus) to run a
  downloaded file.
- Obscures the true nature of a change behind a misleading or reused commit message.

**If you discover such a clone:**

1. Do **not** download or run anything from it.
2. Report it to us via a [GitHub Security Advisory](https://github.com/jhonsferg/gvm-rs/security/advisories/new)
   on this repository, or by opening contact through the maintainer's GitHub profile
   ([@jhonsferg](https://github.com/jhonsferg)), including the repository URL and any
   indicators you noticed.
3. We will independently verify the finding (git history divergence, static analysis, and
   third-party AV/threat-intel confirmation where applicable) and, once confirmed, file an abuse
   report with GitHub requesting takedown of the malicious repository and suspension of the
   distributing account.
4. Confirmed cases will be publicly disclosed (with technical evidence) once the takedown is
   resolved, to help the broader community recognize the pattern.

We do not control third-party accounts or repositories and cannot guarantee removal timelines,
but every credible report will be investigated and escalated to GitHub Trust & Safety.
