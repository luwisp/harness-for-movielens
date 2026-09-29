import contextlib
import io
import json
import pathlib
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src-tauri" / "resources"))
import govern


class GovernanceTests(unittest.TestCase):
    def config(self):
        catalog = json.loads((ROOT / "src-tauri" / "resources" / "rule_catalog.json").read_text())
        return {"entries": {spec["id"]: {"enabled": spec["default_enabled"], "value": spec.get("default_value")}
                            for spec in catalog}}

    def run_clean(self, config):
        sample = [("0", "1::F::25::4::12345"), ("0", "1::F::25::4::12345"),
                  ("1", "1::Film (2000)::Action|Action"),
                  ("2", "1::1::5::1000000000"), ("2", "2::1::5::1000000000")]
        original = govern.rows
        govern.rows = lambda: iter(sample)
        output = io.StringIO()
        try:
            with contextlib.redirect_stdout(output):
                govern.clean(config)
        finally:
            govern.rows = original
        return [(tag, json.loads(body)) for tag, body in (line.split("\t", 1) for line in output.getvalue().splitlines())]

    def test_removal_and_normalization_carry_rule_ids(self):
        items = self.run_clean(self.config())
        actions = [value for tag, value in items if tag == "A"]
        self.assertTrue(any(x["rules"] == ["unique_users"] for x in actions))
        self.assertTrue(any(x["rules"] == ["rating_user_exists"] for x in actions))
        self.assertTrue(any(x["action"] == "normalized" and "normalize_genres" in x["rules"] for x in actions))

    def test_turning_off_one_duplicate_rule_keeps_that_duplicate(self):
        config = self.config()
        config["entries"]["unique_users"]["enabled"] = False
        items = self.run_clean(config)
        users = [v for tag, v in items if tag == "C" and v["table"] == "0"]
        self.assertEqual(len(users), 2)
        self.assertFalse(any("unique_users" in v["rules"] for tag, v in items if tag == "A"))


if __name__ == "__main__":
    unittest.main()
