#!/usr/bin/env python3
"""Checks scripts/deploy.sh runs on AWS's answers before applying a deploy
(docs/plans/2026-10-02-deployment-operating-guide.md §2, step 4).

    deploy_checks.py changes <describe-change-set.json>
        Prints each change that replaces or removes a resource; exit 2 if any.
    deploy_checks.py cors <processed-template.json>
        Prints each API whose CORS block isn't a CORS object; exit 2 if any.

Both read the JSON the AWS CLI prints. Exit 1 means the input couldn't be
read, which deploy.sh treats as a failed check.
"""

import json
import sys

CORS_KEY = "x-amazon-apigateway-cors"


def risky_changes(change_set):
    """Changes that remove a resource, or replace it (or may: "Conditional").
    A replaced table or bucket loses its data."""
    for change in change_set.get("Changes", []):
        rc = change["ResourceChange"]
        if rc["Action"] == "Remove" or rc.get("Replacement") in ("True", "Conditional"):
            what = "removes" if rc["Action"] == "Remove" else f"replaces ({rc.get('Replacement')})"
            yield f"{what} {rc['LogicalResourceId']} ({rc['ResourceType']})"


def cors_shape_problems(value, where):
    """A CORS block must be an object with allowOrigins, or a condition whose
    branches all are. On 2026-10-02 SAM produced a bare list of origins
    instead, and API Gateway quietly kept its old settings."""
    if isinstance(value, dict) and set(value) == {"Fn::If"}:
        _, when_true, when_false = value["Fn::If"]
        yield from cors_shape_problems(when_true, f"{where} (condition true)")
        yield from cors_shape_problems(when_false, f"{where} (condition false)")
    elif isinstance(value, dict) and "allowOrigins" in value:
        return
    elif isinstance(value, dict):
        yield f"{where}: CORS object has no allowOrigins (keys: {sorted(value)})"
    else:
        yield f"{where}: CORS is a {type(value).__name__}, not a CORS object"


def cors_problems(template):
    for name, resource in template.get("Resources", {}).items():
        body = resource.get("Properties", {}).get("Body")
        if isinstance(body, dict) and CORS_KEY in body:
            yield from cors_shape_problems(body[CORS_KEY], name)


def load(path):
    data = json.load(open(path, encoding="utf-8"))
    # get-template returns TemplateBody as a string or as an object.
    return json.loads(data) if isinstance(data, str) else data


def main(argv):
    if len(argv) != 3 or argv[1] not in ("changes", "cors"):
        print(__doc__, file=sys.stderr)
        return 1
    try:
        document = load(argv[2])
    except (OSError, json.JSONDecodeError) as e:
        print(f"couldn't read {argv[2]}: {e}", file=sys.stderr)
        return 1
    problems = list(risky_changes(document) if argv[1] == "changes" else cors_problems(document))
    for problem in problems:
        print(problem)
    return 2 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
