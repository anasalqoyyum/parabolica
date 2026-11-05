"""Validate the task graph and render the committed GitHub issue drafts."""

import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DIRECTORY = ROOT / "docs/implementation"
PLAN = json.loads((DIRECTORY / "plan.json").read_text())
TASKS = PLAN["tasks"]
BY_KEY = {task["key"]: task for task in TASKS}
REPOSITORY = f"https://github.com/{PLAN['repository']}"
FOOTER = f"Posted by {PLAN['author_model']} on behalf of Anas Alqoyyum."


def ancestors(key, visiting=None):
    visiting = set() if visiting is None else visiting
    if key in visiting:
        raise ValueError(f"Dependency cycle at {key}")
    result = set()
    for parent in BY_KEY[key]["blocked_by"]:
        result.add(parent)
        result.update(ancestors(parent, visiting | {key}))
    return result


def reference(key, title=True):
    task = BY_KEY[key]
    label = f"#{task['number']}"
    if title:
        label += f" {task['title']}"
    return f"[{label}]({REPOSITORY}/issues/{task['number']})"


def bullets(items, checklist=False):
    prefix = "- [ ] " if checklist else "- "
    return "\n".join(prefix + item for item in items)


def section(title, content):
    return f"## {title}\n\n{content}\n\n"


def render_issue(task):
    body = section("Outcome", task["outcome"])
    body += section("Read before editing", bullets([
        f"[Product requirements]({REPOSITORY}/blob/master/PRODUCT.md)",
        f"[Technical specification]({REPOSITORY}/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.",
        f"[Design rules]({REPOSITORY}/blob/master/DESIGN.md) and [design handoff]({REPOSITORY}/blob/master/docs/design/README.md) for user-visible work.",
        f"[Agent workflow]({REPOSITORY}/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.",
    ]))
    body += section("Blocked by", bullets([reference(key) for key in task["blocked_by"]]) if task["blocked_by"] else "No task prerequisites. Check the external requirements below before claiming completion.")
    parallel = bullets([reference(key) for key in task["parallel_with"]]) if task["parallel_with"] else "No independent pairing is scheduled. Follow the blockers above."
    body += section("Can run in parallel with", parallel + "\n\nA pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.")
    body += section("Ownership", bullets([f"`{path}`" for path in task["ownership"]]) + "\n\nPaths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.")
    body += section("Deliverables", bullets(task["deliverables"]))
    body += section("Acceptance criteria", bullets(task["acceptance"], checklist=True))
    body += section("Validation and completion evidence", bullets(task["validation"]) + "\n\nRecord commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.")
    if task["external_requirement"]:
        body += section("External requirements", task["external_requirement"])
    if task["key"] == "delivery":
        body += section("Complete delivery checklist", bullets([reference(t["key"]) for t in TASKS if t["key"] != "delivery"], checklist=True))
    body += f"Priority: {task['priority']}. All tasks belong to this delivery; priority sets execution order.\n\n{FOOTER}\n"
    return body


def main():
    assert len(BY_KEY) == len(TASKS)
    assert len({t["number"] for t in TASKS}) == len(TASKS)
    for task in TASKS:
        upstream = ancestors(task["key"])
        for peer in task["parallel_with"]:
            assert task["key"] in BY_KEY[peer]["parallel_with"], (task["key"], peer)
            assert peer not in upstream and task["key"] not in ancestors(peer), (task["key"], peer)
    issues = DIRECTORY / "issues"
    issues.mkdir(exist_ok=True)
    for task in TASKS:
        (issues / f"{task['number']:02}-{task['key']}.md").write_text(render_issue(task))

    index = "# Implementation plan\n\n"
    index += f"{reference('delivery')} tracks the complete delivery. {len(TASKS)} issues cover the player, integration, platform builds and final acceptance. P0/P1 controls order; every listed task must finish in this delivery.\n\n"
    index += section("Start here", f"Start {reference('foundation')} and {reference('discogs-access')} independently. The latter can need external clarification. After #2 merges, #4 becomes available. Each subsequent task starts only after its own blockers are implemented, merged and verified.\n\nRead [the agent workflow](AGENT_WORKFLOW.md) before assigning work. Issue status labels describe the initial scheduling snapshot; the live merged implementation determines readiness.")
    index += section("Dependencies and parallel work", "The table lists direct blockers. Linked parallel pairings are conditional on both tasks' prerequisites and exclusive file ownership. Tasks absent from a pairing should run sequentially unless their owners establish a safe boundary.\n\n| Issue | Priority | Blocked by | Can run in parallel with |\n| --- | --- | --- | --- |\n" + "\n".join(f"| {reference(t['key'])} | {t['priority']} | {', '.join(reference(k, False) for k in t['blocked_by']) or 'None'} | {', '.join(reference(k, False) for k in t['parallel_with']) or 'None'} |" for t in TASKS))
    coverage = [
        ("Workspace, Kit and contracts", ["foundation", "storage", "shell"]),
        ("Lossless FLAC, other local formats, gapless and queue restoration", ["local-playback", "queue"]),
        ("WASAPI, ASIO, FiiO K3 and audio transparency", ["wasapi", "asio", "signal-path"]),
        ("Album/track ReplayGain, tags and peak protection", ["scan", "gain"]),
        ("Local library, file tags/artwork and CUE", ["library", "tags", "cue"]),
        ("YouTube Music account, unified library, playback and discovery", ["credentials", "youtube-access", "youtube-library", "youtube-playback", "youtube-discovery"]),
        ("Discogs per-album release matching, tags and artwork", ["discogs-access", "discogs-adapter", "discogs-import"]),
        ("Windows controls, recovery, accessibility and performance", ["windows-integration", "recovery", "accessibility", "performance"]),
        ("Smart playlists/ratings, sleep timer, lyrics and scrobbling", ["collections", "sleep", "lyrics", "scrobbling"]),
        ("Backup/import/export, packaging and desktop ports", ["data-transfer", "windows-package", "macos", "linux"]),
        ("Full integrated acceptance and deliverable artifacts", ["acceptance", "delivery"]),
    ]
    index += section("Requirement coverage", "| Requirement | Issues |\n| --- | --- |\n" + "\n".join(f"| {name} | {', '.join(reference(k, False) for k in keys)} |" for name, keys in coverage))
    index += section("Completion rules", "Each feature includes the real app entry point, backend, persistence where required, expected failures and meaningful validation. Infrastructure tasks deliver tested contracts for their consumers. Real account, hardware, data-use and platform evidence remains required; fixtures do not replace it.\n\nYouTube access and Discogs durable data/artwork use are explicit blockers. If an external requirement cannot be met, keep the affected issue and delivery tracker open and request the concrete missing input or product decision. Windows is the primary platform; the macOS/Linux tasks are also part of this issue set. Mobile/web targets remain undefined.\n\nDo not close the tracker until #35 proves the combined app and final installable artifacts. No task is deferred to an unnamed next milestone.")
    index += section("Maintain this plan", "Edit [plan.json](plan.json), then run `python3 scripts/render_issue_plan.py` from the repository root. The renderer checks cycles, symmetric pairings and pairs that contradict dependency order. Committed [issue drafts](issues/) are the publication source. After publication, [github-issues.json](github-issues.json) records the actual GitHub IDs and URLs.\n\nIf scope or dependencies change during implementation, update the source plan, regenerate the documents and synchronize the affected GitHub issues and native dependencies. Preserve completion evidence when updating issue bodies.")
    (DIRECTORY / "README.md").write_text(index)
    edges = sum(len(t["blocked_by"]) for t in TASKS)
    pairs = sum(len(t["parallel_with"]) for t in TASKS) // 2
    print(f"Validated {len(TASKS)} issues, {edges} dependency edges, {pairs} symmetric parallel pairings")


if __name__ == "__main__":
    main()
