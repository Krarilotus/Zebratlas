#!/usr/bin/env python3
"""Controlled launcher standard-handle regression; no API/provider/store use."""
import os
import importlib.util
from pathlib import Path
import subprocess
import sys
import unittest
import tempfile


def launcher_module():
    spec = importlib.util.spec_from_file_location("zebra_launcher", Path(__file__).with_name("start-zebra-backend.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class ProviderEnvironment(unittest.TestCase):
    def test_both_key_sources_and_explicit_files_preserve_operator_precedence(self):
        module = launcher_module()
        with tempfile.TemporaryDirectory() as directory:
            parent=Path(directory);root=parent/"repo";root.mkdir();data=root/"data"
            (parent/".env").write_text("OPENROUTER_API_KEY=parent-fixture\nANTHROPIC_API_KEY=parent-anthropic\nRARE_ATLAS_GRAPH_SNAPSHOT=unsafe-parent\n")
            (root/".env").write_text("ANTHROPIC_API_KEY=repo-anthropic\nKISSKI_API_KEY=repo-kisski\nGEMINI_API_KEY=repo-gemini\nRESEND_API_KEY=unrelated-email\nRARE_ATLAS_ACCOUNTS_DB=unsafe-repo\nUnrelated application note without environment assignment\n")
            first=parent/"first.env";first.write_text("ANTHROPIC_API_KEY=explicit-first\n")
            last=parent/"last.env";last.write_text("ANTHROPIC_API_KEY=explicit-last\n")
            loaded=module.provider_environment({"OPENROUTER_API_KEY":"operator-fixture","KISSKI_API_KEY":""},data,[first,last])
            self.assertEqual(loaded["OPENROUTER_API_KEY"],"operator-fixture")
            self.assertEqual(loaded["ANTHROPIC_API_KEY"],"explicit-last")
            self.assertEqual(loaded["KISSKI_API_KEY"],"")
            self.assertEqual(loaded["GEMINI_API_KEY"],"repo-gemini")
            for unrelated in ("RESEND_API_KEY","RARE_ATLAS_GRAPH_SNAPSHOT","RARE_ATLAS_ACCOUNTS_DB"):
                self.assertNotIn(unrelated,loaded)

    def test_configured_provider_key_name_and_literal_values_are_not_executed(self):
        module=launcher_module()
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);config=root/"providers.toml"
            config.write_text('[connections.custom]\nkey_env="CUSTOM_PROVIDER_TOKEN"\n')
            (root/".env").write_text('export CUSTOM_PROVIDER_TOKEN = "literal#$(no-execution)" # comment\n')
            explicit=root/"runtime.env";explicit.write_text(f'ATLAS_LLM_CONFIG="{config.as_posix()}"\n')
            loaded=module.provider_environment({},root/"data",[explicit])
            self.assertEqual(loaded["CUSTOM_PROVIDER_TOKEN"],"literal#$(no-execution)")

    def test_malformed_source_error_contains_no_secret_line(self):
        module=launcher_module()
        with tempfile.TemporaryDirectory() as directory:
            source=Path(directory)/"bad.env";source.write_text('ANTHROPIC_API_KEY="private-fixture-value\n')
            with self.assertRaises(SystemExit) as caught:
                module.read_environment(source)
            self.assertNotIn("private-fixture",str(caught.exception))


class LauncherHandles(unittest.TestCase):
    @unittest.skipUnless(os.name == "nt", "Windows native standard-handle regression")
    def test_redirected_parent_preserves_child_output_without_native_handles(self):
        launcher = Path(__file__).with_name("start-zebra-backend.py").resolve()
        child_source = (
            "import sys; "
            "assert sys.stdin.read() == ''; "
            "sys.stdout.write('controlled-child-stdout\\n'); "
            "sys.stderr.write('controlled-child-stderr\\n')"
        )
        # The redirected Python streams retain their valid file descriptors.
        # Remove only this hidden test parent's native standard-handle pointers
        # to reproduce CREATE_NO_WINDOW inheritance loss in nested children.
        parent_source = f"""
import ctypes, importlib.util, os, sys, subprocess
spec = importlib.util.spec_from_file_location('zebra_launcher', {str(launcher)!r})
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
set_handle = ctypes.windll.kernel32.SetStdHandle
set_handle.argtypes = [ctypes.c_uint32, ctypes.c_void_p]
set_handle.restype = ctypes.c_int
for identifier in (-10, -11, -12):
    assert set_handle(identifier & 0xffffffff, None)
child = module.start_child([sys.executable, '-c', {child_source!r}],
                           env=os.environ.copy(), flags=subprocess.CREATE_NO_WINDOW)
raise SystemExit(child.wait(timeout=10))
"""
        result = subprocess.run(
            [sys.executable, "-c", parent_source], stdin=subprocess.DEVNULL,
            capture_output=True, text=True, timeout=15,
            creationflags=subprocess.CREATE_NO_WINDOW,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "controlled-child-stdout\n")
        self.assertEqual(result.stderr, "controlled-child-stderr\n")


if __name__ == "__main__":
    unittest.main()
