"""Check GitHub issue dependencies, or add missing links with --apply."""

import argparse
import json
import subprocess
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DIRECTORY = ROOT / "docs/implementation"


def github(path, payload=None):
    command = ["gh", "api", path, "-H", "X-GitHub-Api-Version: 2026-03-10"]
    if payload is not None:
        command += ["--method", "POST", "--input", "-"]
    result = subprocess.run(
        command,
        input=json.dumps(payload) if payload is not None else None,
        text=True,
        capture_output=True,
        check=True,
    )
    return json.loads(result.stdout)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    plan = json.loads((DIRECTORY / "plan.json").read_text())
    manifest = json.loads((DIRECTORY / "github-issues.json").read_text())
    assert plan["repository"] == manifest["repository"]
    published = {issue["key"]: issue for issue in manifest["issues"]}
    assert len(published) == len(plan["tasks"])
    prefix = f"repos/{plan['repository']}/issues"

    def inspect(task):
        issue = published[task["key"]]
        actual = github(f"{prefix}/{issue['number']}/dependencies/blocked_by?per_page=100")
        return task["key"], {blocker["id"] for blocker in actual}

    with ThreadPoolExecutor(max_workers=4) as workers:
        current = dict(workers.map(inspect, plan["tasks"]))

    expected = {
        task["key"]: {published[key]["id"] for key in task["blocked_by"]}
        for task in plan["tasks"]
    }
    added = 0
    for task in plan["tasks"]:
        key = task["key"]
        unexpected = current[key] - expected[key]
        if unexpected:
            raise ValueError(f"Unexpected blockers for {key}: {unexpected}; inspect before changing")
        for blocker_id in sorted(expected[key] - current[key]):
            if not args.apply:
                raise ValueError(f"Missing dependencies for {key}; use --apply to add them")
            number = published[key]["number"]
            github(f"{prefix}/{number}/dependencies/blocked_by", {"issue_id": blocker_id})
            added += 1
            if added % 10 == 0:
                print(f"Added {added} missing dependency links", flush=True)
            time.sleep(1.1)

    if args.apply:
        with ThreadPoolExecutor(max_workers=4) as workers:
            current = dict(workers.map(inspect, plan["tasks"]))
    assert current == expected, "GitHub dependencies differ from the source plan"
    count = sum(map(len, current.values()))
    print(f"Verified {count} native dependency links across {len(current)} issues", flush=True)


if __name__ == "__main__":
    main()
