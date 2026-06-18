# Research: GitHub Pull-Request integration

**Status:** research only — no code written yet. Reference this when we pick up
GitHub PR support as a feature.
**Date:** 2026-06-19
**Question:** How do desktop Git clients integrate GitHub pull requests? Do they
shell out to `gh`, or call the API directly? What should kagi do?

---

## TL;DR

- **Nobody shells out to `gh`.** Every desktop client calls the **GitHub HTTP API
  directly** (REST v3 and/or GraphQL v4) with its **own OAuth token**. `gh` is
  itself just a CLI wrapper over the same API — a GUI has no reason to depend on
  an external binary (distribution, version, PATH resolution, output parsing).
- **Auth is OAuth, stored in the OS keychain.** The desktop-friendly choice is the
  **OAuth Device Flow** (no redirect server, and the token exchange needs only the
  public `client_id` — no `client_secret` baked into the binary). A **Personal
  Access Token (PAT)** is the usual fallback.
- **For kagi specifically:** mirror the existing `src/update/` + avatar-fetch
  pattern — **hand-rolled REST over `ureq`** (blocking + rustls), pure models in
  `crates/kagi-domain`, a single `src/github/` (forge) HTTP layer, tokens in the
  OS keychain via the `keyring` crate, UI calls through the Backend (never HTTP
  from `src/ui/`). **Avoid `octocrab`** — it pulls in tokio, which fights gpui's
  executor.

---

## How the reference clients do it

### GitButler (open source — verified from source)
- **Stack:** Tauri = Rust backend + Svelte/TypeScript frontend.
- **Where the forge code lives:** the **frontend (TS)**, under
  `apps/desktop/src/lib/forge/github/` (`githubUserService`, `hooks`, `errorMap`,
  …). It uses **octokit** (GitHub's official JS SDK, REST + GraphQL). There is
  **no `octocrab`** in the Rust side — PR list/create/status all go through
  octokit, i.e. **direct GitHub API calls**.
- **Auth:** **OAuth Device Flow** is the *recommended* path (shows a code, you
  approve in the browser); **PAT** and **GitHub Enterprise** (custom base URL) are
  alternatives. Uses a dedicated registered GitHub OAuth App.
- **Scopes/permissions:** Metadata read + **Pull Requests read/write**.
- **Token storage:** encrypted in the **OS-native keychain**, local-only (never
  leaves the machine).
- **Features:** open/create a PR from a branch, auto-detect the PR associated with
  a branch, check PR status, AI-generated PR text.

### Fork (closed source — from docs + behaviour)
- Connect a GitHub account via **OAuth (browser authorization)** → obtains an
  access token → **calls the GitHub API directly**. PAT also supported. (SSH keys
  are for git transport auth, a separate concern from PR integration.)
- **No `gh` dependency.**

### Industry pattern (for breadth)
- **GitHub Desktop** (Electron) — octokit.
- **VS Code "GitHub Pull Requests" extension** — octokit, leans heavily on
  **GraphQL** to fetch PR + checks + reviews in one round-trip.
- **GitKraken / Sourcetree** — own OAuth App + direct API.
- Common denominator: **OAuth Device Flow or Authorization-Code+PKCE / custom URI
  scheme**, tokens in the **OS keychain**, API called directly.

### Why Device Flow for desktop
- No need to run a localhost redirect server.
- The token-exchange `POST /login/oauth/access_token` (`grant_type=device_code`)
  needs only the **public `client_id`** — **no `client_secret`** — so nothing
  secret has to be embedded in a distributed GUI binary.

---

## Recommended approach for kagi

kagi already has all the building blocks; this maps onto existing patterns.

### 1. Auth — OAuth Device Flow (+ PAT fallback)
- Register one GitHub **OAuth App**; embed only the public `client_id`.
- Flow: `POST https://github.com/login/device/code` → show `user_code` +
  `verification_uri` → poll `POST https://github.com/login/oauth/access_token`
  with `grant_type=urn:ietf:params:oauth:grant-type:device_code` until authorized.
- Offer **manual PAT entry** as the second option (same two-tier setup GitButler
  uses), and a base-URL field later for GitHub Enterprise.

### 2. Token storage — OS keychain
- Add the **`keyring`** crate (macOS Keychain / Windows Credential Manager / Linux
  Secret Service). Do **not** store the token in plaintext `settings.json`.

### 3. API — hand-rolled REST over `ureq`
- kagi already does **`ureq` (blocking + rustls) + hand-scanned JSON** in
  `src/update/` and the avatar fetch. Stay consistent.
- **Avoid `octocrab`** — it's async/**tokio**-based and clashes with the gpui
  executor model kagi uses.
- Endpoints to start with:
  - list: `GET /repos/{owner}/{repo}/pulls`
  - create: `POST /repos/{owner}/{repo}/pulls` (`title`, `body`, `base`, `head`,
    `draft`)
  - richer detail (PR + checks + reviews in one call): **GraphQL v4** later, like
    VS Code.
- Resolve `{owner}/{repo}` from the `origin` remote URL (ssh or https form).

### 4. Layering (follow existing ADRs)
- **`crates/kagi-domain`**: pure PR models + JSON parsing (same shape as
  `kagi_domain::update`). No HTTP, no git2, no gpui.
- **`src/github/`** (or `src/forge/`): the **only** layer that does GitHub HTTP —
  the same立て付け as `src/remote/` (ssh) and `src/update/` (update fetch).
- **`src/ui/`**: calls through the Backend; never makes HTTP directly (consistent
  with the "UI is git2-free" CI gate philosophy — extend the spirit to HTTP).
- Worth an **ADR** when we start (sits alongside ADR-0082 auto-update and
  ADR-0089 remote-over-SSH).

### Minimal first step (lowest risk, real value)
**Device Flow auth + "list PRs for the current branch" + "create one PR"
(title/body/base/head).** Everything else (reviews, checks, merge, comments) layers
on top later.

---

## Sources
- GitButler — GitHub Integration: <https://docs.gitbutler.com/features/forge-integration/github-integration>
- gitbutlerapp/gitbutler source (`apps/desktop/src/lib/forge/github/`, octokit): <https://github.com/gitbutlerapp/gitbutler>
- octokit.js (GitHub JS SDK, REST+GraphQL): <https://github.com/octokit/octokit.js>
- octocrab (Rust GitHub client, tokio-based): <https://github.com/XAMPPRocky/octocrab>
- GitHub Docs — Authorizing OAuth apps / Device Flow: <https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps>
- GitHub Docs — Authenticating to the REST API: <https://docs.github.com/en/rest/authentication/authenticating-to-the-rest-api>
- GitHub Docs — Managing personal access tokens: <https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens>
- Fork — GitHub credentials explainer: <https://gitscripts.com/github-credentials-for-fork-git-client>
