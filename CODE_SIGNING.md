# Code signing policy

CCT has applied to the [SignPath Foundation](https://signpath.org/) program for open-source
projects (#36). Once approved, the Windows release executables will carry this attribution:

> Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by
> [SignPath Foundation](https://signpath.org/).

Until then, releases are unsigned. Every release still ships `SHA256SUMS.txt` and a signed
build provenance attestation; see *Verifying a release* in [SECURITY.md](SECURITY.md).

## What gets signed

- Only `claude-codex-taskbar.exe` built by this repository's
  [release workflow](.github/workflows/release.yml) on GitHub Actions, from a version tag on
  `main`.
- Nothing built on a personal machine is ever signed.
- No third-party binaries are signed. The executable is built from this repository's source
  and the Rust crates pinned in `Cargo.lock`.

## Team roles

| Role | Members |
|---|---|
| Committers and reviewers | [@chiarorosa](https://github.com/chiarorosa) |
| Approvers (release signing) | [@chiarorosa](https://github.com/chiarorosa) |

External contributions are reviewed by a maintainer before merge. `main` is protected:
changes land through pull requests that pass CI. All team members use multi-factor
authentication on GitHub and SignPath.

## Privacy

CCT does not transfer any information to other networked systems except the usage endpoints
of the providers you enable (Anthropic for Claude Code, OpenAI for Codex), using your existing
CLI logins. It has no telemetry and no update check. See [Privacy](README.md#privacy) in the
README.
