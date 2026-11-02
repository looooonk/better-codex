import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import run_source


class SourceLaunchTests(unittest.TestCase):
    def test_builds_helpers_once_and_launches_reported_path_with_arguments(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "custom target" / "development"
            output.mkdir(parents=True)
            binaries = {name: output / f"{name}.exe" for name in run_source.BINARIES}
            for binary in binaries.values():
                binary.touch()
            events = [{"reason": "compiler-artifact", "target": {"name": name}, "executable": str(binary)}
                      for name, binary in binaries.items()]
            with (patch.object(run_source, "prepare", return_value={"PINNED_V8": "yes"}),
                  patch.object(run_source.subprocess, "Popen") as popen,
                  patch.object(run_source.os, "execv") as execute,
                  patch.object(run_source.sys, "argv", ["run_source.py", "--profile", "development", "--",
                                                        "exec", "prompt with spaces", "--model", "test-model"])):
                cargo = popen.return_value.__enter__.return_value
                cargo.stdout = io.StringIO("\n".join(map(json.dumps, events)))
                cargo.wait.return_value = 0
                run_source.main()
                popen.assert_called_once()
                command = popen.call_args.args[0]
                self.assertEqual(command[:5], ["cargo", "build", "--profile", "development", "--message-format=json-render-diagnostics"])
                self.assertEqual([command[index + 1] for index, flag in enumerate(command) if flag == "--bin"],
                                 list(run_source.BINARIES))
                self.assertEqual(popen.call_args.kwargs["env"]["PINNED_V8"], "yes")
                binary = str(binaries["codex"])
                execute.assert_called_once_with(binary, [binary, "exec", "prompt with spaces", "--model", "test-model"])

    def test_failed_build_does_not_launch_stale_binary(self):
        with (patch.object(run_source, "prepare", return_value={}),
              patch.object(run_source.subprocess, "Popen") as popen,
              patch.object(run_source.os, "execv") as execute,
              patch.object(run_source.sys, "argv", ["run_source.py", "--profile", "dev"])):
            cargo = popen.return_value.__enter__.return_value
            cargo.stdout = io.StringIO("")
            cargo.wait.return_value = 101
            self.assertEqual(run_source.main(), 101)
            execute.assert_not_called()


if __name__ == "__main__":
    unittest.main()
