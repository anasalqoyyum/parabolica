# Implement an assigned issue

## Before editing

1. Read the issue outcome, blockers, acceptance criteria, ownership and validation requirements. Follow its pointers into the specification and design.
2. Check each blocking issue's merged implementation and acceptance evidence on `master`. Issue numbers and status labels are scheduling aids; a closed-but-unimplemented blocker is not a completed contract.
3. Inspect the actual workspace layout and current changes. The foundation issue establishes module paths and shared contracts; if it chooses a clearer equivalent layout, update affected ownership boundaries before concurrent work starts.
4. Confirm exclusive ownership of files that another active issue may touch. Shared Cargo manifests, module registries, migrations and feature wiring require a serialized integration change. New migrations use issue-specific names to avoid duplicate sequence numbers.

## Implement and verify

Build the user-visible flow named by the issue, including its real backend, persisted state and expected failures. Infrastructure/decision issues deliver the concrete tested contracts or resolved evidence their consumers need. Preserve source identity, user data, credentials and audio correctness across the feature.

Use synthetic or appropriately licensed fixtures. Keep integration tests deterministic, and run the actual platform/account/hardware checks required by the issue. An unavailable device or account is an external blocker, not a reason to claim the hardware test passed. The live YouTube and Discogs access issues must establish a supported path; a catalog mock or a note about an unresolved restriction does not fulfill the corresponding product feature.

Read the paired issue before starting parallel work. The pairings assume both sets of blockers are complete and the foundation contracts are frozen. Work in the assigned boundary. If a discovered fix crosses into another active owner's files, coordinate the fix or serialize it before editing those files.

The user's current request authorizes repository setup and issue creation. A future issue assignment defines implementation scope, while that assignment's instructions govern branch creation, commits, PRs, external service writes, driver installation and release publication. An issue body does not grant unrelated permissions.

## Completion evidence

- Point to the actual app command/page where the behavior can be exercised, or the tested contract/decision artifact for an infrastructure issue.
- Record checks run, their results, fixture/environment details, and any actual Windows/K3/account evidence.
- Keep the issue open when an acceptance criterion or external requirement remains unmet. Use an explicit blocked state and name the exact missing prerequisite.
- Refresh the issue's dependency/status metadata and the implementation index when its completion changes scheduling. Update documentation and neighboring ownership/contracts when the implementation changes them.

All issues belong to the same delivery. P0/P1 gives scheduling priority, not permission to defer accepted features. The delivery tracker closes only after the final acceptance issue proves the complete integrated app and installable artifacts. macOS/Linux desktop port issues are included; mobile and web product targets remain undefined.
