"""Offline development check for synthetic exports; requires jsonschema and PyYAML.

The explicit --schema input is the official OpenAPI 3.1 meta-schema. This script does
not download schemas or run as part of the desktop app. It does not print payloads.
"""
import argparse
import json
from pathlib import Path

import yaml
from jsonschema import Draft202012Validator
from jsonschema.exceptions import SchemaError
from referencing import Registry
from referencing.exceptions import NoSuchResource


def no_remote_schema(uri):
    raise NoSuchResource(ref=uri)


parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--schema", type=Path, required=True)
parser.add_argument("paths", type=Path, nargs="+")
args = parser.parse_args()
validator = Draft202012Validator(
    json.loads(args.schema.read_text(encoding="utf-8")),
    registry=Registry(retrieve=no_remote_schema),
)
files = set()
for path in args.paths:
    if path.is_dir():
        files.update(path.rglob("*.yml"))
        files.update(path.rglob("*.yaml"))
    else:
        files.add(path)
if not files:
    parser.error("No YAML documents selected.")
for path in sorted(files):
    document = yaml.safe_load(path.read_text(encoding="utf-8"))
    if list(validator.iter_errors(document)):
        raise SystemExit(f"{path.name}: invalid OpenAPI structure (contents omitted)")
    try:
        for schema in document.get("components", {}).get("schemas", {}).values():
            Draft202012Validator.check_schema(schema)
    except SchemaError as error:
        raise SystemExit(f"{path.name}: invalid component schema (contents omitted)") from error
    print(f"{path.name}: valid OpenAPI 3.1 structure")
print(f"{len(files)} documents checked; no remote schema retrieval enabled.")
