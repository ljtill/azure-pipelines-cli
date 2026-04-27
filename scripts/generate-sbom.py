#!/usr/bin/env python3
"""Generates a dependency-free CycloneDX SBOM from cargo metadata JSON."""

from __future__ import annotations

import json
import sys
import urllib.parse
import uuid
from pathlib import Path
from typing import Any


def purl(package: dict[str, Any]) -> str:
    name = urllib.parse.quote(package["name"], safe="")
    version = urllib.parse.quote(package["version"], safe="")
    return f"pkg:cargo/{name}@{version}"


def licenses(package: dict[str, Any]) -> list[dict[str, dict[str, str] | str]]:
    license_expr = package.get("license")
    if license_expr:
        return [{"expression": license_expr}]
    return []


def component(package: dict[str, Any], component_type: str) -> dict[str, Any]:
    item: dict[str, Any] = {
        "type": component_type,
        "bom-ref": package["id"],
        "name": package["name"],
        "version": package["version"],
        "purl": purl(package),
    }
    package_licenses = licenses(package)
    if package_licenses:
        item["licenses"] = package_licenses
    if package.get("repository"):
        item["externalReferences"] = [
            {
                "type": "vcs",
                "url": package["repository"],
            }
        ]
    return item


def dependency_graph(metadata: dict[str, Any]) -> list[dict[str, Any]]:
    nodes = metadata.get("resolve", {}).get("nodes", [])
    package_ids = {package["id"] for package in metadata["packages"]}
    dependencies = []
    for node in nodes:
        ref = node["id"]
        if ref not in package_ids:
            continue
        dependencies.append(
            {
                "ref": ref,
                "dependsOn": [
                    dep["pkg"]
                    for dep in node.get("deps", [])
                    if dep.get("pkg") in package_ids
                ],
            }
        )
    return dependencies


def build_bom(metadata: dict[str, Any]) -> dict[str, Any]:
    packages = {package["id"]: package for package in metadata["packages"]}
    root_id = metadata.get("resolve", {}).get("root")
    if root_id is None or root_id not in packages:
        raise ValueError("cargo metadata did not include a resolvable root package")

    root = packages[root_id]
    bom_uuid = uuid.uuid5(
        uuid.NAMESPACE_URL,
        f"{root.get('repository', root['name'])}@{root['version']}",
    )
    return {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "serialNumber": f"urn:uuid:{bom_uuid}",
        "version": 1,
        "metadata": {
            "component": component(root, "application"),
            "tools": [
                {
                    "vendor": "azure-devops-cli",
                    "name": "scripts/generate-sbom.py",
                }
            ],
        },
        "components": [
            component(package, "library")
            for package_id, package in sorted(packages.items())
            if package_id != root_id
        ],
        "dependencies": dependency_graph(metadata),
    }


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: generate-sbom.py <cargo-metadata.json>", file=sys.stderr)
        return 2

    metadata = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    bom = build_bom(metadata)
    json.dump(bom, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
