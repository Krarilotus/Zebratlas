"""Adversarial checks for release-final and upload refusal, with no network calls."""
import hashlib
import importlib.util
import json
import subprocess
import sys
import tempfile
import types
import unittest
from pathlib import Path
from unittest.mock import patch

from finalize_release import seal
from release_gate import Gate, check_cff, check_secrets, locator, sha256
import validate_release

REPO = Path(__file__).resolve().parents[3]


class FinalGateTest(unittest.TestCase):
    def test_chained_identity_requires_accepted_component_receipts(self):
        gate = Gate.__new__(Gate)
        gate.identity = {"manifest_sha256": "a" * 64,
                         "representatives": {"MONDO:1": "MONDO:1", "ORPHA:1": "MONDO:1", "MONDO:2": "MONDO:2"},
                         "decisions": {"decision:1": {"assertion": "assertion:1", "representative": "MONDO:1"}}}
        row = {"subject_id": "ORPHA:1", "object_id": "MONDO:1", "mapping_justification": "semapv:MappingChaining",
               "rule_id": "R-DIS-08", "rule_version": "1.0.0", "identity_gate_manifest_sha256": "a" * 64,
               "identity_assertion_ids": '["assertion:1"]', "identity_decision_ids": '["decision:1"]'}
        gate.check_identity_mapping(row)
        for mutation in [{"identity_gate_manifest_sha256": "b" * 64}, {"object_id": "MONDO:2"},
                         {"identity_assertion_ids": '["assertion:other"]'}, {"identity_decision_ids": '["decision:unknown"]'},
                         {"identity_decision_ids": '[]'}, {"mapping_justification": "semapv:ManualMappingCuration"},
                         {"identity_assertion_ids": '["assertion:1", "assertion:1"]',
                          "identity_decision_ids": '["decision:1", "decision:1"]'}]:
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                gate.check_identity_mapping(row | mutation)

    def test_formats_only_cannot_issue_a_deployment_receipt(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            receipt = root / "validation.json"
            with (
                patch.object(sys, "argv", ["validator", str(root / "release"),
                                           "--formats-only", "--receipt", str(receipt)]),
                patch.object(validate_release, "Gate") as gate,
            ):
                with self.assertRaisesRegex(ValueError, "deployment receipt requires every gate"):
                    validate_release.main()
                gate.assert_not_called()
            self.assertFalse(receipt.exists())

    def test_containing_records_and_suppression_hashes(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "cache").mkdir()
            (root / "cache/quarantine.json").write_text(json.dumps({"records": [
                {"cache_file": "data/cache/example.json", "record_locator": "/records/1/field"}
            ]}), encoding="utf-8")
            salt = "zebratlas-dev-suppression-salt-v1"
            hashed = hashlib.sha256(salt.encode() + b"\x1fnode:PMID:42").hexdigest()
            (root / "suppression.json").write_text(json.dumps({"schema": "suppression", "version": 1, "entries": [{"id": "sup_fixture", "keys": [hashed], "scope": "all", "reason": "fixture", "date": "2026-10-04T00:00:00Z", "reviewer": "agent:test", "salt_id": hashlib.sha256(salt.encode()).hexdigest()[:8]}]}), encoding="utf-8")
            gate = Gate(root)
            self.assertTrue(gate.blocked(["PMID:42"]))
            self.assertTrue(gate.blocked(refs=[{"cache_file": "cache/example.json", "record_locator": "records[1]/field"}]))
            self.assertFalse(gate.blocked(["PMID:43"]))
            self.assertEqual(locator("line:158/"), "line:158/")
            self.assertEqual(locator("line:158"), "L158")
            with self.assertRaises(ValueError):
                gate.check_snapshot({"quarantine": {"present": True, "manifest_sha256": "bad"}})

    def test_unknown_or_malformed_suppression_cannot_be_ignored(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for value in [{}, {"entries": [{}]}, {"entries": [{"id_sha256": "bad"}]},
                          {"entries": [{"name_affiliation_hash": "unsupported"}]},
                          {"entries": [{"cache_file": "../outside", "record_locator": "x"}]}]:
                (root / "suppression.json").write_text(json.dumps(value), encoding="utf-8")
                with self.assertRaises(ValueError):
                    Gate(root)

    def test_shared_whole_file_quarantine_and_foreign_salt(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "cache").mkdir()
            (root / "cache/quarantine.json").write_text(json.dumps({"entries": [
                {"file": "example.json"}, {"file": "lines.jsonl", "locator": 7}
            ]}))
            gate = Gate(root)
            self.assertTrue(gate.blocked(refs=[{"cache_file": "cache/example.json", "record_locator": "records[99]"}]))
            self.assertTrue(gate.blocked(refs=[{"cache_file": "cache/lines.jsonl", "record_locator": "line:7"}]))
            self.assertFalse(gate.blocked(refs=[{"cache_file": "cache/lines.jsonl", "record_locator": "L8"}]))
            (root / "suppression.json").write_text(json.dumps({"schema": "suppression", "version": 1,
                "entries": [{"id": "sup_fixture", "keys": ["a" * 64], "reason": "fixture", "date": "2026-10-04",
                             "reviewer": "agent:test", "salt_id": "foreign"}]}))
            with self.assertRaises(ValueError):
                Gate(root)

    def test_secret_scan_matches_across_chunks_and_private_env_without_disclosure(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "payload"
            for payload in [b"hf_" + b"A" * 30, b"-----BEGIN " + b"PRIVATE KEY-----", b"sk-proj-" + b"b" * 30]:
                path.write_bytes(b" " * ((1 << 20) - 5) + payload)
                with self.assertRaises(ValueError):
                    check_secrets(path)
            secret = "private-fixture-value-123456"
            env_path = Path(temp) / ".env"
            env_path.write_text("HF_TOKEN=" + secret + "\n", encoding="utf-8")
            path.write_text(secret, encoding="utf-8")
            with self.assertRaises(ValueError) as caught:
                check_secrets(path, env_path)
            self.assertNotIn(secret, str(caught.exception))
            path.write_text("sk-source-identifier-with-several-hyphens", encoding="utf-8")
            check_secrets(path)

    def test_cff_placeholders_validate_locally_and_block_publication(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "CITATION.cff").write_bytes(b'cff-version: 1.2.0\nmessage: Cite the synthetic test dataset.\ntype: dataset\ntitle: Synthetic publication gate fixture\nlicense: CC-BY-4.0\nauthors:\n  - given-names: Johannes\n    family-names: Fixture\n')
            (root / "README.md").write_bytes(b'---\nlicense: cc-by-4.0\ntags: [fixture]\nsize_categories: [n<1K]\n---\nSynthetic gate fixture.\n')
            self.assertTrue(check_cff(root, publishing=True)["cff_schema_validated"])
            citation = (root / "CITATION.cff").read_text().replace("given-names: Johannes", "given-names: '{AUTHOR_NAME}'")
            (root / "CITATION.cff").write_text(citation)
            with self.assertRaises(ValueError):
                check_cff(root, publishing=True)

    def test_seal_covers_every_payload_and_detects_changed_bytes(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "release-report.json").write_text('{"files": {}}', encoding="utf-8")
            (root / "README.md").write_text("fixture", encoding="utf-8")
            seal(root)
            manifest = dict(line.split("  ", 1)[::-1] for line in (root / "SHA256SUMS").read_text().splitlines())
            self.assertEqual(set(manifest), {"README.md", "release-report.json"})
            self.assertEqual(manifest["README.md"], sha256(root / "README.md"))
            (root / "README.md").write_text("modified", encoding="utf-8")
            self.assertNotEqual(manifest["README.md"], sha256(root / "README.md"))

    def test_failed_validation_prevents_token_read_and_hub_import(self):
        spec = importlib.util.spec_from_file_location("publish_fixture", REPO / "scripts/publish_release.py")
        publisher = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(publisher)
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "SHA256SUMS").write_text("corrupt", encoding="utf-8")
            with (
                patch.object(publisher.subprocess, "run", side_effect=subprocess.CalledProcessError(1, "validator")) as run,
                patch("dotenv.dotenv_values") as read_token,
            ):
                with self.assertRaises(subprocess.CalledProcessError):
                    publisher.publish("fixture/dataset", root)
                read_token.assert_not_called()
                run.assert_called_once()
            self.assertNotIn("huggingface_hub", sys.modules)

    def test_public_verification_rejects_extra_files_and_corrupt_download(self):
        spec = importlib.util.spec_from_file_location("verify_fixture", REPO / "scripts/publish_release.py")
        publisher = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(publisher)
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            payload = root / "README.md"
            payload.write_text("public fixture")
            (root / "SHA256SUMS").write_text(f"{sha256(payload)}  README.md\n")
            api = types.SimpleNamespace(repo_info=lambda *a, **k: types.SimpleNamespace(private=False),
                                        list_repo_files=lambda *a, **k: ["README.md", "SHA256SUMS", "extra.json"])
            fake = types.SimpleNamespace(HfApi=lambda **k: api, hf_hub_download=lambda repo, name, **k: str(root / name))
            with patch.dict(sys.modules, {"huggingface_hub": fake}):
                with self.assertRaisesRegex(ValueError, "inventory mismatch"):
                    publisher.verify_published("fixture/dataset", "revision", root)
                api.list_repo_files = lambda *a, **k: ["README.md", "SHA256SUMS", ".gitattributes"]
                payload.write_text("corrupt download")
                with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                    publisher.verify_published("fixture/dataset", "revision", root)


if __name__ == "__main__":
    unittest.main()
