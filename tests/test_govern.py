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

    def run_clean(self, config, sample=None):
        if sample is None:
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

    def run_assess(self, config, sample):
        original = govern.rows
        govern.rows = lambda: iter(sample)
        output = io.StringIO()
        try:
            with contextlib.redirect_stdout(output):
                govern.assess(config)
        finally:
            govern.rows = original
        return json.loads(output.getvalue().split("\t", 1)[1])

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

    def test_missing_delimiter_has_its_own_reason(self):
        items = self.run_clean(self.config(), [("2", "17,34,5,978159683")])
        removed = [v for tag, v in items if tag == "A" and v["action"] == "removed"]
        self.assertEqual(removed[0]["rules"], ["schema_delimiter", "schema_fields"])

    def test_optional_id_ranges_and_year_limit(self):
        config = self.config()
        for name in ("user_id_range", "movie_id_range", "movie_title_year", "movie_title_year_max"):
            config["entries"][name]["enabled"] = True
        sample = [("0", "6041::F::25::4::12345"),
                  ("1", "3953::Film (2004)::Action"),
                  ("1", "3952::Film::Action")]
        removed = [v for tag, v in self.run_clean(config, sample) if tag == "A" and v["action"] == "removed"]
        self.assertEqual(removed[0]["rules"], ["user_id_range"])
        self.assertEqual(removed[1]["rules"], ["movie_id_range", "movie_title_year_max"])
        self.assertEqual(removed[2]["rules"], ["movie_title_year"])

    def test_invalid_row_does_not_claim_duplicate_key(self):
        sample = [("0", "1::F::25::4::12345"), ("0", "1::X::25::4::12345")]
        items = self.run_clean(self.config(), sample)
        removed = [v for tag, v in items if tag == "A" and v["action"] == "removed"]
        kept = [v for tag, v in items if tag == "C"]
        self.assertEqual(removed[0]["rules"], ["user_gender_domain"])
        self.assertEqual(len(kept), 1)
        self.assertEqual(kept[0]["line"], sample[0][1])

    def test_positive_rating_is_independent_of_rating_range(self):
        config = self.config()
        config["entries"]["rating_range"]["enabled"] = False
        sample = [("0", "1::F::25::4::12345"), ("1", "1::Film (2000)::Action"),
                  ("2", "1::1::0::1000000000")]
        removed = [v for tag, v in self.run_clean(config, sample) if tag == "A" and v["action"] == "removed"]
        self.assertEqual(removed[0]["rules"], ["rating_integer"])

    def test_cohort_counts_distinct_movies_without_removing_users(self):
        sample = [("0", "1::F::25::4::12345"), ("0", "2::M::35::5::12345")]
        sample += [("1", f"{i}::Film {i} (2000)::Action") for i in range(1, 21)]
        sample += [("2", f"1::{i}::5::1000000000") for i in range(1, 21)]
        sample += [("2", f"2::1::5::{1000000000 + i}") for i in range(20)]
        config = self.config()
        result = self.run_assess(config, sample)
        self.assertEqual(result["score_spec_version"], "quality-spec-v2")
        self.assertEqual(result["cohort20"]["qualified_users"], 1)
        self.assertEqual(result["cohort20"]["evaluable_users"], 2)
        self.assertEqual(result["dimensions"]["complete"]["tables"]["0"]["good"], 1)
        cleaned = self.run_clean(config, sample)
        kept_users = [value["line"] for tag, value in cleaned if tag == "C" and value["table"] == "0"]
        self.assertEqual(len(kept_users), 2)

    def test_assessment_does_not_follow_optional_cleaning_range(self):
        sample = [("0", "6041::F::25::4::12345")]
        result = self.run_assess(self.config(), sample)
        self.assertEqual(result["dimensions"]["accurate"]["tables"]["0"]["good"], 0)
        self.assertEqual(result["dimensions"]["accurate"]["tables"]["0"]["eligible"], 1)

    def test_consistency_marks_entire_conflicting_group(self):
        config = self.config()
        sample = [("0", "1::F::25::4::12345"), ("0", "1::M::25::4::12345")]
        first = self.run_assess(config, sample)
        second = self.run_assess(config, list(reversed(sample)))
        for result in (first, second):
            self.assertEqual(result["dimensions"]["consistent"]["tables"]["0"]["good"], 0)
            self.assertEqual(result["dimensions"]["consistent"]["tables"]["0"]["eligible"], 2)

    def test_historical_window_accepts_february_2003_but_rejects_milliseconds(self):
        sample = [("2", "1::1::5::1046476799"),
                  ("2", "1::1::5::1046476800"),
                  ("2", "1::1::5::1046476799000")]
        result = self.run_assess(self.config(), sample)
        time = result["dimensions"]["up_to_date"]["tables"]["2"]
        self.assertEqual((time["good"], time["eligible"]), (1, 3))


if __name__ == "__main__":
    unittest.main()
