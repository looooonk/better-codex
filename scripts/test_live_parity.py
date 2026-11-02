#!/usr/bin/env python3
"""Bounded, isolated live-model checks for a built Better Codex executable."""

import argparse
import contextlib
import json
import os
from pathlib import Path
import platform
import selectors
import shutil
import signal
import subprocess
import tempfile
import time
import uuid


MAX_BYTES = 16 * 1024 * 1024


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def stop_owned_process(process):
    if process.stdin and not process.stdin.closed:
        process.stdin.close()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        with contextlib.suppress(ProcessLookupError):
            os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            with contextlib.suppress(ProcessLookupError):
                os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=5)


class Rpc:
    def __init__(self, binary, environment, workspace, timeout):
        self.timeout = timeout
        self.stderr = tempfile.TemporaryFile()
        self.process = subprocess.Popen(
            [str(binary), "app-server", "--listen", "stdio://"],
            cwd=workspace, env=environment, stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=self.stderr, start_new_session=True,
        )
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.process.stdout, selectors.EVENT_READ)
        self.buffer = b""
        self.events = []
        self.responses = {}
        self.sequence = 0
        self.bytes_read = 0
        self.unexpected_requests = []
        try:
            self.metadata = self.request("initialize", {
                "clientInfo": {"name": "better_codex_live_parity", "version": "1.0"},
                "capabilities": {"experimentalApi": True},
            })
            self.send({"method": "initialized", "params": {}})
        except BaseException:
            self.close()
            raise

    def close(self):
        stop_owned_process(self.process)
        self.selector.close()
        self.process.stdout.close()
        self.stderr.close()

    def send(self, value):
        self.process.stdin.write((json.dumps(value) + "\n").encode())
        self.process.stdin.flush()

    def receive(self, deadline):
        while b"\n" not in self.buffer:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not self.selector.select(remaining):
                raise TimeoutError("app-server response deadline exceeded")
            data = os.read(self.process.stdout.fileno(), 65536)
            if not data:
                self.stderr.seek(0, os.SEEK_END)
                self.stderr.seek(max(0, self.stderr.tell() - 2000))
                raise RuntimeError("app-server closed: " + self.stderr.read().decode(errors="replace"))
            self.bytes_read += len(data)
            require(self.bytes_read <= MAX_BYTES, "app-server exceeded bounded output budget")
            self.buffer += data
        line, self.buffer = self.buffer.split(b"\n", 1)
        value = json.loads(line)
        if "id" in value and "method" in value:
            self.unexpected_requests.append(value["method"])
            self.send({"id": value["id"], "error": {
                "code": -32601, "message": "This isolated test does not grant interactive approvals."
            }})
        elif "id" in value:
            self.responses[value["id"]] = value
        else:
            self.events.append(value)
            require(len(self.events) <= 20000, "app-server exceeded event budget")
        return value

    def request(self, method, params):
        self.sequence += 1
        request_id = self.sequence
        self.send({"id": request_id, "method": method, "params": params})
        deadline = time.monotonic() + self.timeout()
        while request_id not in self.responses:
            self.receive(deadline)
        response = self.responses.pop(request_id)
        if "error" in response:
            raise RuntimeError(f"{method}: {response['error']}")
        return response["result"]

    def wait_turn(self, thread_id, turn_id, start, expected="completed"):
        deadline = time.monotonic() + self.timeout()
        index = start
        while True:
            for event in self.events[index:]:
                params = event.get("params", {})
                turn = params.get("turn", {})
                if (event.get("method") == "turn/completed"
                        and params.get("threadId") == thread_id and turn.get("id") == turn_id):
                    require(turn.get("status") == expected, f"turn status {turn.get('status')}: {turn.get('error')}")
                    require(not self.unexpected_requests, f"unexpected server requests: {self.unexpected_requests}")
                    events = [event for event in self.events[start:]
                              if event.get("params", {}).get("threadId") == thread_id]
                    items = [event["params"]["item"] for event in events
                             if event.get("method") == "item/completed"
                             and event.get("params", {}).get("threadId") == thread_id]
                    messages = [item for item in items if item.get("type") == "agentMessage"]
                    final = [item for item in messages if item.get("phase") == "final_answer"]
                    text = "\n".join(item.get("text", "") for item in final or messages)
                    return {"text": text, "items": items, "events": events, "turn_id": turn_id}
            index = len(self.events)
            self.receive(deadline)

    def turn(self, thread_id, prompt, **extra):
        start = len(self.events)
        response = self.request("turn/start", {"threadId": thread_id,
            "input": [{"type": "text", "text": prompt, "textElements": []}], **extra})
        return self.wait_turn(thread_id, response["turn"]["id"], start)


def tool_names(result):
    names = set()
    for item in result["items"]:
        kind = item.get("type", "")
        if kind not in {"agentMessage", "userMessage", "reasoning"}:
            names.add(kind)
        for field in ("name", "tool", "kind"):
            if isinstance(item.get(field), str):
                names.add(item[field])
    for event in result["events"]:
        item = event.get("params", {}).get("item", {})
        if item.get("type") in {"function_call", "custom_tool_call"}:
            names.add(item.get("name", ""))
    return sorted(names)


class Suite:
    def __init__(self, args, root):
        self.args = args
        self.root = root
        self.home = root / "home"
        self.workspace = root / "workspace"
        self.home.mkdir(mode=0o700)
        self.workspace.mkdir()
        self.deadline = time.monotonic() + args.total_timeout
        self.env = os.environ.copy()
        for key in list(self.env):
            if key.startswith("CODEX_") and key not in {"CODEX_API_KEY", "CODEX_ACCESS_TOKEN", "CODEX_CA_CERTIFICATE"}:
                del self.env[key]
        self.env.update(CODEX_HOME=str(self.home), CODEX_SQLITE_HOME=str(self.home),
                        OMP_NUM_THREADS="1", OPENBLAS_NUM_THREADS="1", RUST_LOG="error")
        self.secrets = []
        source = args.auth_source
        if source.is_dir():
            source = source / "auth.json"
        if source.is_file():
            auth = source.read_bytes()
            shutil.copyfile(source, self.home / "auth.json")
            (self.home / "auth.json").chmod(0o600)
            def collect(value):
                if isinstance(value, dict):
                    for key, child in value.items():
                        if isinstance(child, str) and any(word in key.lower() for word in ("token", "key", "secret")):
                            self.secrets.append(child)
                        else:
                            collect(child)
                elif isinstance(value, list):
                    for child in value:
                        collect(child)
            collect(json.loads(auth))
        else:
            require(any(self.env.get(key) for key in ("OPENAI_API_KEY", "CODEX_API_KEY", "CODEX_ACCESS_TOKEN")),
                    "No auth.json or API credential environment variable is available")
        self.secrets.extend(value for key, value in self.env.items()
                            if any(word in key.upper() for word in ("TOKEN", "SECRET", "API_KEY")) and value)
        sandbox = "danger-full-access" if args.yolo else "workspace-write"
        (self.home / "config.toml").write_text(
            f'model = {json.dumps(args.model)}\nmodel_reasoning_effort = "low"\n'
            f'approval_policy = "never"\nsandbox_mode = "{sandbox}"\n'
            'cli_auth_credentials_store = "file"\nsuppress_unstable_features_warning = true\n'
            '[analytics]\nenabled = false\n[features]\ndaemon_auto_start = false\n'
            f'[projects.{json.dumps(str(self.workspace))}]\ntrust_level = "trusted"\n'
        )
        (self.workspace / "AGENTS.md").write_text(
            "This is an isolated regression workspace. Operate only inside it. "
            "Use no GPUs, network tools, package installs, or long-running commands. "
            "Do not spawn agents unless the current test explicitly requests one.\n"
        )
        self.rpc = None
        self.cases = []
        self.models = []
        self.features = []
        self.thread_id = None
        self.marker = "PARITY-" + uuid.uuid4().hex[:10]

    def remaining(self):
        remaining = self.deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("suite deadline exceeded")
        return min(self.args.timeout, remaining)

    def redact(self, message):
        for secret in self.secrets:
            if len(secret) >= 6:
                message = message.replace(secret, "[REDACTED]")
        return message.replace(str(self.root), "[TEST_ROOT]")

    def case(self, name, operation, **context):
        started = time.monotonic()
        try:
            details = operation() or {}
            status = "skip" if details.get("skipped") else "pass"
        except Exception as error:
            status, details = "fail", {"error": self.redact(str(error))[:3000]}
        record = {"name": name, "status": status, "seconds": round(time.monotonic() - started, 3), **context, **details}
        self.cases.append(record)
        print(json.dumps(record), flush=True)
        return status == "pass"

    def connect(self):
        if self.rpc:
            self.rpc.close()
            self.rpc = None
        self.rpc = Rpc(self.args.binary, self.env, self.workspace, self.remaining)

    def new_thread(self, **extra):
        require(self.rpc is not None, "app-server initialization failed")
        params = {"model": self.args.model, "cwd": str(self.workspace),
                  "approvalPolicy": "never", "sandbox": "danger-full-access" if self.args.yolo else "workspace-write",
                  "experimentalRawEvents": True, **extra}
        return self.rpc.request("thread/start", params)["thread"]["id"]

    def catalog(self):
        self.connect()
        require(Path(self.rpc.metadata["codexHome"]).resolve() == self.home.resolve(), "app-server escaped isolated home")
        for method, destination in (("model/list", self.models), ("experimentalFeature/list", self.features)):
            cursor = None
            for _ in range(20):
                response = self.rpc.request(method, {"cursor": cursor, "limit": 100})
                destination.extend(response["data"])
                cursor = response.get("nextCursor")
                if cursor is None:
                    break
            else:
                raise AssertionError(f"{method} exceeded pagination bound")
        require(self.models and self.features, "empty model or feature catalog")
        require(any(model.get("model") == self.args.model or model.get("id") == self.args.model for model in self.models),
                "requested live model absent from catalog")
        return {"model_count": len(self.models), "feature_count": len(self.features),
                "models": [model.get("model", model.get("id")) for model in self.models],
                "server": self.rpc.metadata.get("userAgent")}

    def conversation(self):
        self.thread_id = self.new_thread()
        result = self.rpc.turn(self.thread_id, f"Remember this marker for later: {self.marker}. Reply READY only.")
        require("READY" in result["text"], "new turn did not answer READY")
        return {"thread_id": self.thread_id}

    def resume(self):
        require(self.thread_id, "new-thread test failed")
        self.connect()
        resumed = self.rpc.request("thread/resume", {"threadId": self.thread_id, "cwd": str(self.workspace)})
        require(resumed["thread"]["id"] == self.thread_id, "resume changed identity")
        result = self.rpc.turn(self.thread_id, "Reply with the exact marker I asked you to remember earlier; no tools.")
        require(self.marker in result["text"], "resumed history lost marker")
        return {"persisted_history": True}

    def fork(self):
        require(self.thread_id, "new-thread test failed")
        fork = self.rpc.request("thread/fork", {"threadId": self.thread_id, "cwd": str(self.workspace)})["thread"]["id"]
        require(fork != self.thread_id, "fork reused parent identity")
        result = self.rpc.turn(fork, "Reply with the exact remembered marker; no tools.")
        require(self.marker in result["text"], "fork lost history")
        result = self.rpc.turn(self.thread_id, "Reply ORIGINAL-CONTINUES only; no tools.")
        require("ORIGINAL-CONTINUES" in result["text"], "original did not continue after fork")
        return {"distinct_fork": True, "original_continued": True}

    def file_edit(self):
        thread = self.new_thread()
        (self.workspace / "counter.txt").write_text("41\n")
        result = self.rpc.turn(thread,
            "Use a shell tool to read counter.txt in the current workspace, then edit it to exactly 42 followed by a newline. "
            "Read it back with a shell tool to verify, and reply FILE-VERIFIED. Do not change other files.")
        require((self.workspace / "counter.txt").read_text() == "42\n", "file edit content incorrect")
        tools = tool_names(result)
        require(any(name in tools for name in ("commandExecution", "exec_command", "shell", "shell_command")), "no shell execution observed")
        require("FILE-VERIFIED" in result["text"], "file verification response missing")
        return {"tools": tools, "file_content": "42\\n"}

    def code_mode(self):
        thread = self.new_thread(config={"features.code_mode": True, "features.code_mode_host": True})
        result = self.rpc.turn(thread,
            "Use the JavaScript code execution tool functions.exec (code mode) to evaluate 19 * 23 and print it with text(). "
            "Do not use shell, Python, network, or files. Reply with the computed number only.")
        require("437" in result["text"], "code-mode arithmetic answer incorrect")
        tools = tool_names(result)
        require(any(name == "exec" or name.endswith(".exec") for name in tools), "no code-mode execution observed")
        return {"tools": tools, "result": 437}

    def structured(self):
        result = self.rpc.turn(self.new_thread(), 'Return answer 42 and label "parity". No tools.', outputSchema={
            "type": "object", "properties": {"answer": {"type": "integer"}, "label": {"type": "string"}},
            "required": ["answer", "label"], "additionalProperties": False})
        require(json.loads(result["text"]) == {"answer": 42, "label": "parity"}, "structured output incorrect")
        return {"validated_schema": True}

    def interrupt(self):
        thread = self.new_thread()
        start = len(self.rpc.events)
        turn = self.rpc.request("turn/start", {"threadId": thread, "input": [{"type": "text",
            "text": "Write every integer from 1 through 10000, one per line. Do not use tools.", "textElements": []}]})["turn"]["id"]
        deadline = time.monotonic() + self.remaining()
        while True:
            events = [event for event in self.rpc.events[start:]
                      if event.get("params", {}).get("threadId") == thread
                      and event.get("params", {}).get("turn", {}).get("id") == turn]
            require(not any(event.get("method") == "turn/completed" for event in events),
                    "turn completed before it could be interrupted")
            if any(event.get("method") == "turn/started" for event in events):
                break
            self.rpc.receive(deadline)
        self.rpc.request("turn/interrupt", {"threadId": thread, "turnId": turn})
        self.rpc.wait_turn(thread, turn, start, expected="interrupted")
        result = self.rpc.turn(thread, "The previous request is cancelled. Reply RECOVERED only; no tools.")
        require("RECOVERED" in result["text"], "turn did not recover after interruption")
        return {"interrupted": True, "subsequent_turn_completed": True}

    def multi_agent(self):
        model = next((entry for entry in self.models if entry.get("model", entry.get("id")) == self.args.model), {})
        if model.get("multiAgentVersion") not in {"v1", "v2"}:
            return {"skipped": "requested model does not advertise multi-agent support"}
        result = self.rpc.turn(self.new_thread(),
            f"Spawn exactly one subagent using model {self.args.model} at low reasoning effort. "
            "Ask it to compute 17 + 25 mentally with no tools or file access. Wait for its answer and close it. "
            "Then reply DELEGATED-42. Spawn no further agents and do not use any shell, network, or files.")
        tools = tool_names(result)
        require(any(name in {"collabAgentToolCall", "subAgentActivity", "spawn_agent", "spawnAgent"} for name in tools),
                "no delegation event observed")
        require("DELEGATED-42" in result["text"], "delegated answer incorrect")
        return {"tools": tools}

    def probe_model(self, model, metadata):
        supported = [entry["reasoningEffort"] for entry in metadata.get("supportedReasoningEfforts", [])]
        effort = next((value for value in ("low", "minimal", "none", "medium") if value in supported),
                      metadata.get("defaultReasoningEffort", "low"))
        response = self.rpc.request("thread/start", {
            "model": model, "cwd": str(self.workspace), "approvalPolicy": "never", "sandbox": "read-only",
            "config": {"model_reasoning_effort": effort}, "experimentalRawEvents": True,
        })
        require(response["model"] in {model, metadata.get("model")}, "server selected a different model")
        result = self.rpc.turn(response["thread"]["id"],
            "Compute 23 + 19 mentally. Reply MODEL-PROBE-42 only. Do not call tools or spawn agents.", effort=effort)
        require(result["text"].strip() == "MODEL-PROBE-42", "model probe answer incorrect")
        require(not tool_names(result), "model probe unexpectedly used tools")
        reroutes = [event["params"] for event in result["events"] if event.get("method") == "model/rerouted"]
        require(not reroutes, f"requested model was rerouted: {reroutes}")
        return {"selected_model": response["model"], "reasoning_effort": effort, "inference_completed": True}

    def upstream_roundtrip(self):
        require(self.thread_id, "new-thread test failed")
        require(self.rpc is None, "copy requires all owned app-servers to be stopped")
        source_home = self.root / "roundtrip-source"
        self.home.rename(source_home)
        copied_home = self.home
        # Keep absolute rollout paths valid while preserving the original test data.
        shutil.copytree(source_home, copied_home)
        environment = {**self.env, "CODEX_HOME": str(copied_home), "CODEX_SQLITE_HOME": str(copied_home)}
        marker = "UPSTREAM-" + uuid.uuid4().hex[:10]
        servers = []
        for name, binary in (("upstream", self.args.upstream_binary), ("current", self.args.binary)):
            rpc = Rpc(binary, environment, self.workspace, self.remaining)
            try:
                require(Path(rpc.metadata["codexHome"]).resolve() == copied_home.resolve(),
                        f"{name} app-server escaped copied home")
                read = rpc.request("thread/read", {"threadId": self.thread_id, "includeTurns": True})
                require(read["thread"]["id"] == self.thread_id, f"{name} read changed identity")
                require(self.marker in json.dumps(read["thread"]), f"{name} could not read original history")
                if name == "current":
                    require(marker in json.dumps(read["thread"]), "current server lost upstream-added history")
                resumed = rpc.request("thread/resume", {"threadId": self.thread_id, "cwd": str(self.workspace)})
                require(resumed["thread"]["id"] == self.thread_id, f"{name} resume changed identity")
                prompt = (f"Remember an additional marker {marker}. Reply with the earlier PARITY marker only; no tools."
                          if name == "upstream" else "Reply with both remembered markers, PARITY and UPSTREAM; no tools.")
                result = rpc.turn(self.thread_id, prompt)
                require(self.marker in result["text"], f"{name} model lost original history")
                if name == "current":
                    require(marker in result["text"], "current model lost upstream-added history")
                require(not tool_names(result), f"{name} roundtrip unexpectedly used tools")
                servers.append({"binary": name, "server": rpc.metadata.get("userAgent"),
                                "history_read": True, "resume_inference_completed": True})
            finally:
                rpc.close()
        return {"isolated_copy": True, "servers": servers}

    def cli_exec(self, structured=False):
        output = self.root / ("structured-output.json" if structured else "exec-output.txt")
        command = [str(self.args.binary), "exec", "--json", "--skip-git-repo-check",
                   "--model", self.args.model, "--output-last-message", str(output)]
        if self.args.yolo:
            command.append("--yolo")
        if structured:
            schema = self.root / "output-schema.json"
            schema.write_text(json.dumps({"type": "object", "properties": {"ok": {"type": "boolean"}},
                                          "required": ["ok"], "additionalProperties": False}))
            command.extend(["--output-schema", str(schema), 'Return {"ok":true}. No tools.'])
        else:
            command.append("Reply EXEC-PARITY-OK only. Do not use tools.")
        with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
            process = subprocess.Popen(command, cwd=self.workspace, env=self.env,
                stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr, start_new_session=True)
            try:
                process.wait(timeout=self.remaining())
                require(process.returncode == 0, f"exec exited with status {process.returncode}")
                require(stdout.tell() <= MAX_BYTES, "exec output budget exceeded")
                stdout.seek(0)
                events = [json.loads(line) for line in stdout.read().splitlines() if line.strip()]
                require(any(event.get("type") == "turn.completed" for event in events), "no completed exec turn event")
                text = output.read_text()
                require(json.loads(text) == {"ok": True} if structured else "EXEC-PARITY-OK" in text,
                        "exec final output incorrect")
                return {"json_events": len(events), "structured": structured}
            finally:
                stop_owned_process(process)

    def run(self):
        try:
            self.case("app_server_catalog_and_features", self.catalog)
            self.case("new_thread", self.conversation)
            self.case("resume_after_server_restart", self.resume)
            self.case("fork_and_continue_parent", self.fork)
            self.case("shell_and_file_edit", self.file_edit)
            self.case("code_mode_arithmetic", self.code_mode)
            self.case("structured_app_server_output", self.structured)
            self.case("interrupt_and_recovery", self.interrupt)
            self.case("single_subagent", self.multi_agent)
            for model in dict.fromkeys(self.args.probe_model):
                metadata = next((entry for entry in self.models if model in {entry.get("model"), entry.get("id")}), {})
                self.case("model_probe", lambda model=model, metadata=metadata: self.probe_model(model, metadata),
                          requested_model=model, catalog_metadata=metadata)
            if self.rpc:
                self.rpc.close()
                self.rpc = None
            self.case("exec_json", self.cli_exec)
            self.case("exec_structured_output", lambda: self.cli_exec(structured=True))
            if self.args.upstream_binary:
                self.case("isolated_upstream_roundtrip", self.upstream_roundtrip)
        finally:
            if self.rpc:
                self.rpc.close()
                self.rpc = None
        return self.cases


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--auth-source", type=Path, default=Path(os.environ.get("CODEX_HOME", Path.home() / ".codex")))
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--model", default="gpt-6-luna")
    parser.add_argument("--probe-model", action="append", default=[], help="Additional model for a tiny inference check; repeatable")
    parser.add_argument("--upstream-binary", type=Path, help="Older upstream CLI for an isolated copied-home read/resume roundtrip")
    parser.add_argument("--timeout", type=float, default=180, help="Per-request/turn timeout in seconds")
    parser.add_argument("--total-timeout", type=float, default=1800, help="Maximum suite duration in seconds")
    parser.add_argument("--yolo", action="store_true", help="Use unsandboxed execution only inside the isolated test workspace")
    args = parser.parse_args()
    args.binary = args.binary.resolve(strict=True)
    if args.upstream_binary:
        args.upstream_binary = args.upstream_binary.resolve(strict=True)
    require(args.timeout > 0 and args.total_timeout > 0, "timeouts must be positive")
    report = {"system": platform.system(), "architecture": platform.machine(), "model": args.model,
              "binary": str(args.binary), "sandbox": "danger-full-access" if args.yolo else "workspace-write",
              "probe_models": list(dict.fromkeys(args.probe_model)),
              "upstream_binary": str(args.upstream_binary) if args.upstream_binary else None}
    started = time.monotonic()
    try:
        with tempfile.TemporaryDirectory(prefix="bc-live-", dir="/tmp") as directory:
            report["cases"] = Suite(args, Path(directory).resolve()).run()
    except Exception as error:
        report["setup_error"] = str(error)[:1000]
    report["seconds"] = round(time.monotonic() - started, 3)
    report["passed"] = bool(report.get("cases")) and all(case["status"] != "fail" for case in report["cases"]) and "setup_error" not in report
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"report": str(args.report.resolve()), "passed": report["passed"], "seconds": report["seconds"]}), flush=True)
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
