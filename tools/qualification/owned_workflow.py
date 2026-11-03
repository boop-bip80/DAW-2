#!/usr/bin/env python3
# Copyright (C) 2026 Lemon Studio contributors
# SPDX-License-Identifier: GPL-3.0-or-later
# This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

"""Required, self-owned application fixture: create/save/cold-open/render/recover.

Uses the supplied executable, a fresh user directory and synthetic audio. It never
loads a user's project or requires an installed third-party plugin/audio device.
"""
import argparse
import contextlib
import hashlib
import json
import math
import os
from pathlib import Path
import re
import secrets
import socket
import struct
import subprocess
import time
import wave


def sha256(path):
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def run(binary, output):
    binary = binary.resolve(strict=True)
    output.mkdir(parents=True, exist_ok=False)
    output = output.resolve()
    env = os.environ.copy()
    env["OPENSTUDIO_USER_DIR"] = str(output / "user")
    env["OPENSTUDIO_NO_PLUGIN_BUILD"] = "1"
    env["OS_MIX_THREADS"] = "0"
    tone = output / "owned-tone.wav"
    with wave.open(str(tone), "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(48000)
        wav.writeframes(b"".join(struct.pack("<h", round(0.15 * 32767 * math.sin(2 * math.pi * 110 * n / 48000))) for n in range(24000)))
    session = output / "owned-session.lemon"

    @contextlib.contextmanager
    def application(project=None, label="create"):
        token = secrets.token_hex(24)
        log_path = output / (label + ".log")
        log = log_path.open("w", encoding="utf-8")
        args = [str(binary), "--serve", "0", "--control-token", token]
        if project:
            args.append(str(project))
        process = subprocess.Popen(args, env=env, stdout=log, stderr=log)
        connection = stream = None
        try:
            deadline = time.monotonic() + 60
            while True:
                if process.poll() is not None:
                    raise RuntimeError(f"{label}: executable exited {process.returncode}; see {log_path}")
                found = re.search(r"listening at 127\.0\.0\.1:(\d+)", log_path.read_text(encoding="utf-8", errors="replace"))
                if found:
                    connection = socket.create_connection(("127.0.0.1", int(found[1])), timeout=30)
                    break
                if time.monotonic() >= deadline:
                    raise TimeoutError(f"{label}: control server did not start")
                time.sleep(0.05)
            stream = connection.makefile("rw", encoding="utf-8", newline="\n")
            sequence = 0

            def call(method, **params):
                nonlocal sequence
                sequence += 1
                stream.write(json.dumps({"jsonrpc": "2.0", "id": sequence, "method": method, "params": params}) + "\n")
                stream.flush()
                line = stream.readline()
                if not line:
                    raise RuntimeError("control connection ended before a reply")
                reply = json.loads(line)
                if "error" in reply:
                    raise RuntimeError(f"{method}: {reply['error']}")
                return reply["result"]

            call("hello", name="Owned release qualification", token=token)
            yield call
        finally:
            if stream:
                stream.close()
            if connection:
                connection.close()
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=10)
            log.close()
            # The per-process token is no longer valid and has no place in retained public CI artifacts.
            log_path.write_text(log_path.read_text(encoding="utf-8", errors="replace").replace(token, "[ephemeral token removed]"), encoding="utf-8")

    with application() as call:
        added = call("apply", commands=[{"op": "add_track"}, {"op": "add_pattern", "name": "Owned pattern"}])["results"]
        track, pattern = added[0]["track"], added[1]["pattern"]
        channel = call("apply", command={"op": "add_channel", "name": "Owned sine", "sample": str(tone), "track": track})["results"][0]["channel"]
        commands = [
            {"op": "set_eq", "track": track, "band": 0, "gain": 900, "frequency": 2900, "width": 15000},
            {"op": "set_eq", "track": track, "band": 1, "gain": -450, "frequency": 13000, "width": 18000},
            {"op": "set_eq", "track": track, "band": 2, "gain": 120, "frequency": 47000, "width": 7000},
            {"op": "set_notes", "pattern": pattern, "notes": [{"channel": channel, "position": 0, "length": 96, "key": 60, "velocity": 100}]},
            {"op": "add_clip", "track": 1, "position": 0, "length": 192, "pattern": pattern},
        ]
        call("apply", commands=commands)
        before = call("state")
        call("save", path=str(session))
        assert session.read_bytes().startswith(b"LMhd"), "ordinary native save must contain a native header"
    with application(session, "cold-reopen") as call:
        after = call("state")
    expected = next(t for t in before["mixer"] if t["id"] == track)
    actual = next(t for t in after["mixer"] if t["number"] == expected["number"])
    assert expected["eq"] == actual["eq"], "mixer EQ changed across a cold reopen"
    assert len(before["channels"]) == len(after["channels"])
    assert len(before["patterns"]) == len(after["patterns"])

    renders = []
    for number in (1, 2):
        path = output / f"render-{number}.wav"
        completed = subprocess.run([str(binary), "--render", str(session), str(path), "--rate", "48000", "--seconds", "3", "--builtin-only"],
                                   env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=120)
        (output / f"render-{number}.log").write_text(completed.stdout, encoding="utf-8")
        assert completed.returncode == 0, f"render {number} failed: see its log"
        renders.append(path)
    assert renders[0].read_bytes() == renders[1].read_bytes(), "the owned render must be deterministic in fresh processes"
    data = renders[0].read_bytes()
    assert data[:4] == b"RIFF" and data[8:12] == b"WAVE"
    offset, payload, fmt = 12, None, None
    while offset + 8 <= len(data):
        tag, size = data[offset:offset + 4], struct.unpack_from("<I", data, offset + 4)[0]
        body = data[offset + 8:offset + 8 + size]
        assert len(body) == size, "truncated WAV chunk"
        if tag == b"fmt ":
            fmt = struct.unpack_from("<HHIIHH", body)
        if tag == b"data":
            payload = body
        offset += 8 + size + (size % 2)
    assert fmt and fmt[0] == 3 and fmt[1] == 2 and fmt[2] == 48000 and fmt[5] == 32
    assert payload and len(payload) % 8 == 0
    values = struct.unpack("<" + "f" * (len(payload) // 4), payload)
    assert all(math.isfinite(v) for v in values), "render contains non-finite samples"
    peak = max(map(abs, values))
    assert peak > 0.001, "render is silent"

    part = output / "take.wav.part"
    part.write_bytes(struct.pack("<ff", 0.25, -0.25) * 128)
    journal = Path(str(part) + ".jsonl")
    journal.write_text(json.dumps({"version": 1, "channels": 2, "rate": 48000, "head": 3}) + '\n{"rate":', encoding="utf-8")
    occupied = output / "take.wav"
    occupied.write_bytes(b"previous unrelated recording")
    recovered = subprocess.run([str(binary), "--recover-recording", str(part)], env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=30)
    (output / "recovery.log").write_text(recovered.stdout, encoding="utf-8")
    assert recovered.returncode == 0, "retained recording recovery failed"
    assert occupied.read_bytes() == b"previous unrelated recording"
    fresh = output / "take (recovered 1).wav"
    assert fresh.stat().st_size == 44 + 125 * 8
    assert not part.exists() and not journal.exists()

    report = {
        "status": "passed", "binary_sha256": sha256(binary), "fixture_origin": "synthetic, generated by this repository",
        "executed": ["native-save", "cold-reopen-all-nine-EQ-values", "nonsilent-finite-stereo-render", "deterministic-second-render",
                     "recording-recovery-with-torn-journal", "existing-take-preserved"],
        "executed_count": 6, "skipped_count": 0,
        "render_peak": peak, "render_frames": len(payload) // 8, "project_sha256": sha256(session),
        "not_qualified": ["external FL project/audio parity corpus", "physical audio devices and MIDI loopback", "third-party plugin compatibility", "clean-machine installation", "distribution signatures"],
    }
    (output / "result.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="new directory for fixtures, logs and the result manifest")
    args = parser.parse_args()
    try:
        run(args.binary, args.output)
    except Exception as error:
        args.output.mkdir(parents=True, exist_ok=True)
        (args.output / "failure.json").write_text(json.dumps({"status": "failed", "error": str(error)}, indent=2) + "\n", encoding="utf-8")
        raise


if __name__ == "__main__":
    main()
