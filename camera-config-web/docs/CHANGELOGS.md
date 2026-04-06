2026-04-03 - preserve profile editor focus while renaming
- Stopped rerendering the full profiles list on each profile-name keystroke so the active input keeps focus and the sidebar scroll position stays stable.
- Updated the visible profile card title in place while continuing to refresh dependent camera/profile UI that uses the renamed profile.

2026-04-03 - add profile duplication
- Added a Duplicate action to each profile card to clone free-flow settings and peaks into a new adjacent profile.
- Generated a unique copied profile name automatically so duplication does not create export-blocking name collisions.
