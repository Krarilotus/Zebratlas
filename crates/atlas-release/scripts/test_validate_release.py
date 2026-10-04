"""Adversarial tests for the all-payload privacy scan; no third-party packages needed."""
import importlib.util
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("validator", Path(__file__).with_name("validate_release.py"))
validator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(validator)


class PrivacyScanTest(unittest.TestCase):
    def test_personal_leaks_fail_in_every_payload_format(self):
        attacks = [
            b'{"kind":"person","id":"opaque"}', b'ra:nodeKind "person" .',
            b'ORCID:0000-0001-2345-6789', b'0000-0001-2345-6789',
            b'https://orcid.org/0000-0001-2345-6789', b'person%3Aname', b'REPORTER.PI:42',
            b'{"contact_name":"example"}', b'{"pi_name":"example"}',
            b'{"principal_investigator":"example"}', b'{"author":"example"}', b'{"staff_name":"example"}',
            b'ra:authorName "example" .', b'cache/people/overlap.json',
        ]
        with tempfile.TemporaryDirectory() as directory:
            for extension in ["jsonl", "ttl", "tsv", "md", "cff", "json"]:
                path = Path(directory) / f"payload.{extension}"
                for attack in attacks:
                    with self.subTest(extension=extension, attack=attack):
                        # The ORCID/profile markers straddle the one MiB scanner boundary.
                        path.write_bytes(b" " * ((1 << 20) - 5) + attack)
                        with self.assertRaises(ValueError):
                            validator.check_privacy(path)

    def test_nonpersonal_records_and_collective_citation_pass(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "payload"
            path.write_bytes(b'{"kind":"paper","id":"PMID:1"}\nauthors:\n  - name: Rare Disease Atlas contributors\n')
            validator.check_privacy(path)


if __name__ == "__main__":
    unittest.main()
