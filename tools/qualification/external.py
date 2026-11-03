#!/usr/bin/env python3
# Copyright (C) 2026 Lemon Studio contributors
# SPDX-License-Identifier: GPL-3.0-or-later
# This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

"""Fail-closed external compatibility qualification; proprietary fixtures are not bundled."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys


def preflight():
    counts = {}
    for name in ("OPENSTUDIO_FLP_CORPUS", "OPENSTUDIO_FLP_PAIRS", "OS_FX_NULL", "OS_PC_NULL", "OS_FC_NULL"):
        raw = os.environ.get(name)
        if not raw:
            raise ValueError(f"{name} is required for external qualification")
        path = Path(raw).resolve(strict=True)
        if name.startswith("OPENSTUDIO"):
            lines = [line.strip() for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
            if not lines:
                raise ValueError(f"{name} is empty")
            for line in lines:
                paths = line.split("\t") if name.endswith("PAIRS") else [line]
                if name.endswith("PAIRS") and len(paths) != 2:
                    raise ValueError(f"{name} requires original<TAB>resaved paths")
                for entry in paths:
                    if not Path(entry).is_file():
                        raise ValueError(f"{name} references missing file {entry}")
            counts[name] = len(lines)
        else:
            cases = sorted(path.glob("*.case"))
            if not cases:
                raise ValueError(f"{name} contains no .case fixtures")
            counts[name] = len(cases)
    return counts


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    report = {"status": "failed", "profile": "external-compatibility"}
    try:
        report["fixture_counts"] = preflight()
        env = os.environ.copy()
        env["LEMON_REQUIRE_EXTERNAL_FIXTURES"] = "1"
        with (args.output / "external-tests.log").open("w", encoding="utf-8") as log:
            result = subprocess.run(["cargo", "test", "--locked", "-p", "os-flp", "-p", "os-engine", "--", "--nocapture"], env=env, stdout=log, stderr=subprocess.STDOUT)
        report["exit_code"] = result.returncode
        if result.returncode:
            raise RuntimeError("external qualification tests failed; see external-tests.log")
        report["status"] = "passed"
    except Exception as error:
        report["error"] = str(error)
    (args.output / "result.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
