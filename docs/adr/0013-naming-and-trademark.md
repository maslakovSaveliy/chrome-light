# ADR-0013: Codename `chrome-light` and trademark

**Status:** Accepted
**Date:** 2026-09-07
**Deciders:** project owner

## Context

The directory and working name is `chrome-light`. "Chrome" and "Google Chrome" are registered trademarks of Google. A product that imitates Chrome by name creates a risk of claims and misleads users (especially with profile import and "sync").

## Decision

- Product name: **ChromeLight** (owner's decision 2026-09-07). Repository/workspace — `chrome-light`, binary `chromelight`, scheme `chromelight://`, crate prefix `cl-`.
- **Risk accepted by the owner:** "Chrome" is a Google trademark; the name ChromeLight may draw a claim (trademark dilution/confusion). Mitigation: in UI/docs — "ChromeLight is not affiliated with Google"; no Chrome logos/colors; wording such as "import data from Google Chrome", "compatibility with Chrome Web Store extensions". `tools/rename-checklist.md` (M5) remains in case a rename is forced.
- In UI and docs never claim "a Chrome equivalent" or "sync with Chrome".

## Consequences

- Easier: development is not blocked on naming.
- Harder: renaming before release — routine but mandatory work.

## Action Items

1. [x] The owner chose ChromeLight.
2. [ ] `tools/rename-checklist.md` (M5) — in case of a claim.
3. [ ] Disclaimer "not affiliated with Google" in About and README before the public release.
