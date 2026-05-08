# Active-profile plan detection — improvement plan

> Tracking doc for the chain of work uncovered while auditing how the
> dashboard derives `plan_name` and `subscription_expires_at` for each
> account card. See the audit findings at the bottom for context.

## Status legend

- `[ ]` not started
- `[~]` in-flight (PR open, not merged)
- `[x]` merged

## Phase A — zero-risk fixes (one PR)

- [x] **A1.** Persist `wham/usage.plan_type` into `ProfileMetadata`
      Pipe the API-derived plan into `sync_profile_metadata_from_auth_and_quota`,
      preferring it over the id_token claim. Drop the `#[allow(dead_code)]` on
      `ChatGptApiSnapshot.plan_type`. Remove `apply_paid_fallback_for_free_plan`
      once the API path supersedes it.
- [x] **A2.** Top-level `chatgpt_plan_type` fallback in id_token decode
      Mirror CodexBar's defensive fallback: if `auth.chatgpt_plan_type` is
      missing in the nested `https://api.openai.com/auth` claim, look at the
      top-level `chatgpt_plan_type`. One-line guard against schema drift.
- [x] **A3.** Hide misleading "0 days" plan label
      `planLine(plan, daysLeft)` should drop the days suffix when the cached
      `subscription_expires_at` is absent or already past. Optionally surface a
      subtle "needs refresh" hint instead.

PR: #19 (merged 2026-05-08)

## Phase B — proactive freshness (one or two PRs)

- [x] **B1.** Force OAuth refresh on user-initiated card refresh
      `RefreshOptions { force_token_rotation }` plumbed through
      `refresh_profile_via_api_with_options`. mac/win `try_refresh_via_chatgpt_api`
      now sets `force_token_rotation: true`. The 5-min silent ticker keeps the
      cheap path.
- [x] **B2.** Day-rollover background pass over all OAuth profiles
      New Tauri command `refresh_all_oauth_profile_plans_silent` walks every
      OAuth profile, forces a token rotation, and rolls plan + quota into
      metadata. Frontend kicks it on bootstrap (after first dashboard render)
      and on every local-day rollover (10-min polling against
      `Date.toDateString()`).
- [x] **B3.** Track `last_plan_check_ms` separately from `last_refresh` and
      `quota_updated_at_ms`, so the UI can show plan freshness independently of
      quota freshness. Stamped by `sync_profile_metadata_from_auth*` whenever a
      plan is confirmed (id_token claim or API plan_type).

PR: _(to be created)_

## Phase C — UX polish (independent of A/B)

- [x] **C1.** Plan badge on each card with hover-time freshness tooltip.
      `last_plan_check_ms` now flows from `ProfileMetadata` →
      `ProfileIndexEntry` → `ProfileCard`/`CurrentCard`. Front-end
      `planFreshnessTitle` renders a localized "Plan tier confirmed N
      min/h/d ago" tooltip, and `isPlanCheckStale` (>36h) drives a
      subtle leading dot via the `.plan-check-stale` CSS class.
- [x] **C2.** Replace silent "free → paid" fallback with explicit
      `unknown_paid` state. Backend constant renamed to
      `UNKNOWN_PAID_PLAN_NAME`. Front-end maps the token to a localized
      "Unknown paid plan" label with a "Re-login to confirm" hint
      surfaced in the same hover tooltip; the warning hue dot via
      `.plan-unknown-paid` separates it from a plain stale cache.

PR: _(to be created)_

## Phase D — architecture

- [x] **D1.** Decouple plan-update path from quota-update path.
      `sync_profile_metadata_from_auth_and_quota` removed. `sync_profile_quota`
      and `sync_profile_metadata_from_auth(profile, api_plan_override, home)`
      are now the only two entry points. Callers that previously bundled
      both arguments now make two writes; the operations touch disjoint
      `ProfileMetadata` fields so order is irrelevant. Disk cost is one
      extra ~1KB write per refresh, which is invisible in practice.
- [~] **D2.** ~~Investigate `/accounts/check/v4-2023-04-27` as a more
      authoritative plan/subscription endpoint than `/wham/usage`.~~
      **Dropped.** The path's hard-coded `2023-04-27` date suffix telegraphs
      that it's a versioned snapshot endpoint OpenAI may rotate / retire
      without notice; reverse-engineering it carries ongoing maintenance
      risk that A1's `/wham/usage.plan_type` already neutralizes.

## Audit findings (context)

### Data flow (current, 1.5.4)

```
auth.json
  └─ tokens.id_token (JWT, decoded URL-safe base64)
       └─ "https://api.openai.com/auth" claim
            ├─ chatgpt_plan_type             →  ProfileMetadata.plan_name
            └─ chatgpt_subscription_active_until  →  ProfileMetadata.subscription_expires_at
```

`ChatGptApiSnapshot.plan_type` from `/wham/usage` IS populated on every silent
refresh but never persisted (`#[allow(dead_code)]`). That is the freshest
plan signal we have today.

### Update triggers

| Trigger | Cadence | Profiles touched | Path |
|---|---|---|---|
| Silent ticker | every 5 min, only if quota >5min stale | active OAuth only | `commands/dashboard.rs::refresh_active_profile_quota_silent` → `chatgpt_api::refresh_profile_via_api` → `metadata::sync_profile_metadata_from_auth_and_quota` |
| User Refresh button | on click | one card | `refresh_runtime::refresh_profile` |
| User Login button | on click | one card | `login_runtime::login_profile_with_home` → `metadata::sync_profile_metadata_from_auth` |
| User Switch button | on click | active changes | indirect via profiles_index reload |
| App startup | once | all (lazy hydrate) | `metadata::hydrate_profile_metadata` |

`refresh_oauth_tokens` only fires when access_token is expired (~60min TTL)
or after a 401, so the id_token in `auth.json` rarely rotates for an
otherwise-quiet active profile and never rotates for inactive profiles.

### Observed staleness on the maintainer's machine (2026-05-08)

| Folder | Plan | `subscription_expires_at` | Age |
|---|---|---|---|
| a | plus | 2026-04-13 | -25d |
| b | team | 2026-04-27 | -11d |
| c | team | 2026-04-27 | -11d |
| d | pro | 2026-04-08 | -30d |
| e | pro (active) | 2026-05-04 | -4d |

Active profile `e` is only 4 days stale because the silent ticker eventually
caught it; inactive profiles are weeks stale because nothing refreshes them
until the user clicks Refresh / Login / Switch.

### Reference projects

- `steipete/CodexBar` — uses the same `id_token.https://api.openai.com/auth.chatgpt_plan_type` source. Has an extra fallback: if missing, pulls top-level `chatgpt_plan_type` from the JWT payload. Does **not** consume `wham/usage.plan_type` for plan derivation.
- `farion1231/cc-switch` — config-only switcher, no quota / plan logic.
- `Cmochance/codex-app-transfer` — protocol-forwarding tool, not OAuth-account-aware.

So **A1** (persist API plan_type) is the optimization industry peers haven't taken; **A2** (top-level fallback) is borrowing CodexBar's defensiveness.
