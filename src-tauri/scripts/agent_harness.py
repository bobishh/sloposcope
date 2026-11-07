#!/usr/bin/env python3
import json
import os
import pty
import select
import subprocess
import sys
from pathlib import Path


def _bool_env(name: str) -> bool:
    value = os.environ.get(name, "").strip().lower()
    return value in {"1", "true", "yes", "on"}


def _json_env(name: str, default):
    raw = os.environ.get(name)
    if raw is None or raw.strip() == "":
        return default
    try:
        return json.loads(raw)
    except Exception:
        return default


def _render(value, replacements):
    rendered = str(value)
    for key, replacement in replacements.items():
        rendered = rendered.replace("{" + key + "}", replacement)
    return rendered


class JsonRpcClient:
    def __init__(self, argv):
        self._process = subprocess.Popen(
            argv,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        self._id = 0
        self._initialize()

    def _request(self, method, params):
        self._id += 1
        payload = {
            "jsonrpc": "2.0",
            "id": self._id,
            "method": method,
            "params": params,
        }
        assert self._process.stdin is not None
        assert self._process.stdout is not None
        self._process.stdin.write(json.dumps(payload) + "\n")
        self._process.stdin.flush()
        line = self._process.stdout.readline()
        if not line:
            stderr = ""
            if self._process.stderr is not None:
                stderr = self._process.stderr.read()
            raise RuntimeError(f"MCP server closed pipe. stderr={stderr.strip()}")
        response = json.loads(line)
        if response.get("error"):
            raise RuntimeError(response["error"].get("message") or "Unknown MCP error")
        return response.get("result")

    def _initialize(self):
        self._request("initialize", {"protocolVersion": 1, "clientInfo": {"name": "sloposcope-agent-harness", "version": "0.1.0"}})

    def tool(self, name, arguments):
        return self._request("tools/call", {"name": name, "arguments": arguments})

    def close(self):
        try:
            if self._process.stdin:
                self._process.stdin.close()
        except Exception:
            pass
        try:
            self._process.terminate()
            self._process.wait(timeout=2)
        except Exception:
            try:
                self._process.kill()
            except Exception:
                pass


def _drain_nudges(nudge_path, offset):
    if not nudge_path:
      return offset, []
    path = Path(nudge_path)
    if not path.is_file():
      return offset, []
    try:
      with path.open("r", encoding="utf-8") as handle:
        handle.seek(offset)
        lines = [line.rstrip("\n") for line in handle.readlines()]
        return handle.tell(), [line for line in lines if line.strip()]
    except OSError:
      return offset, []


def _run_with_pty(argv, cwd, env, prompt_text, nudge_path):
    master_fd, slave_fd = pty.openpty()
    proc = subprocess.Popen(
        argv,
        cwd=cwd,
        env=env,
        stdin=slave_fd,
        stdout=slave_fd,
        stderr=slave_fd,
        close_fds=True,
    )
    os.close(slave_fd)
    if prompt_text:
        try:
            os.write(master_fd, prompt_text.encode("utf-8"))
            if not prompt_text.endswith("\n"):
                os.write(master_fd, b"\n")
        except OSError:
            pass

    chunks = []
    nudge_offset = 0
    while True:
        nudge_offset, nudges = _drain_nudges(nudge_path, nudge_offset)
        for nudge in nudges:
            try:
                os.write(master_fd, nudge.encode("utf-8"))
                if not nudge.endswith("\n"):
                    os.write(master_fd, b"\n")
            except OSError:
                pass
        ready, _, _ = select.select([master_fd], [], [], 0.1)
        if master_fd in ready:
            try:
                data = os.read(master_fd, 4096)
            except OSError:
                data = b""
            if data:
                chunks.append(data)
            elif proc.poll() is not None:
                break
        elif proc.poll() is not None:
            try:
                data = os.read(master_fd, 4096)
                if data:
                    chunks.append(data)
            except OSError:
                pass
            break
    os.close(master_fd)
    return proc.wait(), b"".join(chunks).decode("utf-8", errors="replace")


def _run_with_pipes(argv, cwd, env, prompt_text):
    proc = subprocess.Popen(
        argv,
        cwd=cwd,
        env=env,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    stdout, stderr = proc.communicate(prompt_text if prompt_text else None)
    return proc.returncode, stdout or "", stderr or ""


def _git_touched_paths(cwd):
    result = subprocess.run(
        ["git", "status", "--porcelain"],
        cwd=cwd,
        capture_output=True,
        text=True,
        check=False,
    )
    paths = []
    for raw in result.stdout.splitlines():
        line = raw.rstrip()
        if len(line) < 4:
            continue
        path = line[3:]
        if " -> " in path:
            path = path.split(" -> ", 1)[1]
        path = path.strip()
        if path and path not in paths:
            paths.append(path)
    return paths


def _git_patch(cwd):
    result = subprocess.run(
        ["git", "diff"],
        cwd=cwd,
        capture_output=True,
        text=True,
        check=False,
    )
    return result.stdout or ""


def _run_validation(cwd, command):
    command = (command or "").strip()
    if not command:
        return {
            "command": "",
            "success": False,
            "exitCode": None,
            "stdout": "",
            "stderr": "validation command missing",
        }
    result = subprocess.run(
        ["/bin/sh", "-lc", command],
        cwd=cwd,
        capture_output=True,
        text=True,
        check=False,
    )
    return {
        "command": command,
        "success": result.returncode == 0,
        "exitCode": result.returncode,
        "stdout": result.stdout or "",
        "stderr": result.stderr or "",
    }


def main():
    diagnostics_path = os.environ.get("SLOPOSCOPE_AGENT_HARNESS_DIAGNOSTICS_PATH", "").strip()
    target_command = os.environ.get("SLOPOSCOPE_TARGET_COMMAND", "").strip()
    if not target_command:
        raise SystemExit("SLOPOSCOPE_TARGET_COMMAND missing")

    bundle_dir = os.environ.get("SLOPOSCOPE_AGENT_BUNDLE_DIR", "").strip()
    context_path = os.environ.get("SLOPOSCOPE_AGENT_CONTEXT_PATH", "").strip()
    prompt_path = os.environ.get("SLOPOSCOPE_AGENT_PROMPT_PATH", "").strip()
    nudge_path = os.environ.get("SLOPOSCOPE_AGENT_NUDGE_PATH", "").strip()
    prompt_text = ""
    if prompt_path and Path(prompt_path).is_file():
        prompt_text = Path(prompt_path).read_text(encoding="utf-8")
    if not _bool_env("SLOPOSCOPE_PROMPT_TO_STDIN"):
        prompt_text = ""

    replacements = {
        "bundle_dir": bundle_dir,
        "prompt_path": prompt_path,
        "instruction_pack_path": os.environ.get("SLOPOSCOPE_AGENT_BASE_INSTRUCTIONS_PATH", ""),
        "mission_facts_path": os.environ.get("SLOPOSCOPE_AGENT_MISSION_FACTS_PATH", ""),
        "objective_contract_path": os.environ.get("SLOPOSCOPE_AGENT_OBJECTIVE_CONTRACT_PATH", ""),
        "session_context_path": os.environ.get("SLOPOSCOPE_AGENT_SESSION_CONTEXT_PATH", ""),
        "active_mcp_config_path": os.environ.get("SLOPOSCOPE_ACTIVE_MCP_CONFIG_PATH", ""),
        "repo_path": os.environ.get("SLOPOSCOPE_REPO_PATH", ""),
        "worktree_path": os.environ.get("SLOPOSCOPE_WORKTREE_PATH", ""),
        "mission_id": os.environ.get("SLOPOSCOPE_MISSION_ID", ""),
        "objective_id": os.environ.get("SLOPOSCOPE_OBJECTIVE_ID", ""),
        "session_id": os.environ.get("SLOPOSCOPE_SESSION_ID", ""),
        "profile_id": os.environ.get("SLOPOSCOPE_PROFILE_ID", ""),
    }

    target_args = [_render(arg, replacements) for arg in _json_env("SLOPOSCOPE_TARGET_ARGS_JSON", [])]
    target_env = dict(os.environ)
    extra_env = _json_env("SLOPOSCOPE_TARGET_ENV_JSON", {})
    if isinstance(extra_env, dict):
        for key, value in extra_env.items():
            target_env[str(key)] = _render(value if isinstance(value, str) else json.dumps(value), replacements)

    argv = [target_command, *target_args]
    cwd = os.environ.get("SLOPOSCOPE_WORKTREE_PATH", os.getcwd())
    transport_kind = os.environ.get("SLOPOSCOPE_PROFILE_TRANSPORT_KIND", "pty").strip() or "pty"
    mcp_command = _json_env("SLOPOSCOPE_ACTIVE_MCP_COMMAND_JSON", [])
    if not isinstance(mcp_command, list):
        mcp_command = []
    mcp_command = [str(item) for item in mcp_command if str(item).strip()]

    mcp_client = None
    mcp_error = None
    mission_id = os.environ.get("SLOPOSCOPE_MISSION_ID", "")
    objective_id = os.environ.get("SLOPOSCOPE_OBJECTIVE_ID", "")
    session_id = os.environ.get("SLOPOSCOPE_SESSION_ID", "")
    profile_id = os.environ.get("SLOPOSCOPE_PROFILE_ID", "")
    validation_command = os.environ.get("SLOPOSCOPE_VALIDATION_COMMAND", "").strip()

    if mcp_command:
        try:
            mcp_client = JsonRpcClient(mcp_command)
            base = {
                "repo_path": os.environ.get("SLOPOSCOPE_REPO_PATH", ""),
                "mission_id": mission_id,
                "objective_id": objective_id,
                "session_id": session_id,
                "profile_id": profile_id,
                "workspace_path": cwd,
            }
            mcp_client.tool("agent_login", {**base, "assignment_kind": "execute_objective", "state": "running"})
            context_snapshot = mcp_client.tool("agent_request_context", {
                **base,
                "event_limit": 8,
            })
            if context_path:
                Path(context_path).parent.mkdir(parents=True, exist_ok=True)
                Path(context_path).write_text(json.dumps(context_snapshot, indent=2), encoding="utf-8")
            mcp_client.tool("agent_activity_set", {**base, "state": "running"})
            mcp_client.tool("agent_report_event", {
                **base,
                "unit_id": objective_id,
                "event_type": "process_started",
                "payload": {
                    "command": target_command,
                    "args": target_args,
                    "bundleDir": bundle_dir,
                    "contextPath": context_path,
                    "promptPath": prompt_path,
                    "transportKind": transport_kind,
                },
            })
        except Exception as err:
            mcp_error = str(err)
            mcp_client = None

    exit_code = 1
    stdout_text = ""
    stderr_text = ""
    combined_output = ""
    execution_error = None
    try:
        if transport_kind == "pty":
            exit_code, combined_output = _run_with_pty(argv, cwd, target_env, prompt_text, nudge_path)
        else:
            exit_code, stdout_text, stderr_text = _run_with_pipes(argv, cwd, target_env, prompt_text)
    except Exception as err:
        execution_error = str(err)
        stderr_text = (stderr_text + ("\n" if stderr_text else "") + execution_error).strip()
        combined_output = (combined_output + ("\n" if combined_output else "") + execution_error).strip()
    finally:
        touched_paths = _git_touched_paths(cwd)
        patch_text = _git_patch(cwd)
        validation = _run_validation(cwd, validation_command)
        verdict_status = "passed" if exit_code == 0 and validation["success"] else "failed"
        verdict_summary = (
            f"Agent session {session_id} completed validation."
            if verdict_status == "passed"
            else f"Agent session {session_id} failed."
        )
        if mcp_client is not None:
            base = {
                "repo_path": os.environ.get("SLOPOSCOPE_REPO_PATH", ""),
                "mission_id": mission_id,
                "objective_id": objective_id,
                "session_id": session_id,
                "profile_id": profile_id,
                "workspace_path": cwd,
            }
            try:
                if touched_paths:
                    mcp_client.tool("agent_report_touched_files", {**base, "touched_paths": touched_paths})
                mcp_client.tool("agent_report_event", {
                    **base,
                    "unit_id": objective_id,
                    "event_type": "process_exited",
                    "payload": {
                        "exitCode": exit_code,
                        "transportKind": transport_kind,
                        "stdoutTail": stdout_text[-2000:],
                        "stderrTail": stderr_text[-2000:],
                        "combinedTail": combined_output[-2000:],
                    },
                })
                mcp_client.tool("agent_report_event", {
                    **base,
                    "unit_id": objective_id,
                    "event_type": "validation_finished",
                    "payload": validation,
                })
                mcp_client.tool("agent_submit_verdict", {
                    **base,
                    "status": verdict_status,
                    "summary": verdict_summary,
                    "details": {
                        "submittedByAgent": True,
                        "kind": "cli_worker",
                        "profileId": profile_id,
                        "workspacePath": cwd,
                        "touchedPaths": touched_paths,
                        "patch": patch_text,
                        "validationStatus": "passed" if validation["success"] else "failed",
                        "validation": validation,
                        "diagnostics": {
                            "transportKind": transport_kind,
                            "exitCode": exit_code,
                            "stdoutTail": stdout_text[-2000:],
                            "stderrTail": stderr_text[-2000:],
                            "combinedTail": combined_output[-2000:],
                        },
                    },
                })
                final_state = "completed" if verdict_status == "passed" else "failed"
                mcp_client.tool("agent_activity_set", {**base, "state": final_state, "touched_paths": touched_paths, "last_error": None if exit_code == 0 else f"exit_code={exit_code}"})
                mcp_client.tool("agent_logout", {**base, "state": final_state, "reason": f"exit_code={exit_code}"})
            except Exception as err:
                if not mcp_error:
                    mcp_error = str(err)
            finally:
                mcp_client.close()

    diagnostics = {
        "targetCommand": target_command,
        "targetArgs": target_args,
        "transportKind": transport_kind,
        "workspacePath": cwd,
        "bundleDir": bundle_dir,
        "contextPath": context_path,
        "promptPath": prompt_path,
        "nudgePath": nudge_path,
        "exitCode": exit_code,
        "stdout": stdout_text,
        "stderr": stderr_text,
        "combinedOutput": combined_output,
        "touchedPaths": touched_paths,
        "patch": patch_text,
        "validation": validation,
        "mcpCommand": mcp_command,
        "mcpError": mcp_error,
        "mcpAvailable": mcp_error is None and bool(mcp_command),
        "executionError": execution_error,
    }
    if diagnostics_path:
        Path(diagnostics_path).parent.mkdir(parents=True, exist_ok=True)
        Path(diagnostics_path).write_text(json.dumps(diagnostics, indent=2), encoding="utf-8")

    raise SystemExit(exit_code)


if __name__ == "__main__":
    main()
