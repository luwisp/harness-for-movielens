"""MovieLens 1M Hadoop Streaming worker. Output tags: C rows, A actions, M summary."""
import json
import os
import re
import sys
from collections import Counter

KINDS = {"users.dat": "0", "movies.dat": "1", "ratings.dat": "2"}
FIELDS = {"0": 5, "1": 3, "2": 4}
AGES = {"1", "18", "25", "35", "45", "50", "56"}
GENRES = {"Action", "Adventure", "Animation", "Children's", "Comedy", "Crime", "Documentary",
          "Drama", "Fantasy", "Film-Noir", "Horror", "Musical", "Mystery", "Romance",
          "Sci-Fi", "Thriller", "War", "Western"}


def emit(tag, value):
    print(tag + "\t" + json.dumps(value, ensure_ascii=True, separators=(",", ":")))


def mapper():
    source = os.environ.get("mapreduce_map_input_file") or os.environ.get("map_input_file", "")
    kind = KINDS.get(os.path.basename(source))
    if kind is None:
        raise RuntimeError("Unknown input file: " + source)
    for line in sys.stdin.buffer:
        raw = line.decode(os.environ.get("ML_INPUT_ENCODING", "iso-8859-1")).rstrip("\r\n")
        encoded = json.dumps(raw, ensure_ascii=True)
        print(kind + "|" + encoded + "\t" + encoded)


def integer(s):
    try:
        value = int(s)
        return value if str(value) == s else None
    except ValueError:
        return None


def enabled(config, name):
    return config["entries"][name]["enabled"]


def parameter(config, name):
    return config["entries"][name]["value"]


def issues(kind, parts, config, raw):
    found = ["schema_delimiter"] if "::" not in raw else []
    if len(parts) != FIELDS[kind]:
        return found + ["schema_fields"]
    if not all(item.strip() for item in parts):
        return found + ["required_values"]
    def check(rule, good):
        if not good:
            found.append(rule)
    if kind == "0":
        uid, gender, age, occupation, zipcode = parts
        user_id = integer(uid)
        check("user_id_positive", user_id is not None and user_id > 0)
        limit = parameter(config, "user_id_range")
        check("user_id_range", user_id is not None and limit["min"] <= user_id <= limit["max"])
        check("user_gender_domain", gender in {"F", "M"})
        check("user_age_domain", age in AGES)
        occupation_code = integer(occupation)
        limit = parameter(config, "user_occupation_range")
        check("user_occupation_range", occupation_code is not None and limit["min"] <= occupation_code <= limit["max"])
        check("user_zip_format", re.fullmatch(r"\d{5}(?:-\d{4})?", zipcode) is not None)
    elif kind == "1":
        mid, title, genres = parts
        values = genres.split("|")
        movie_id = integer(mid)
        check("movie_id_positive", movie_id is not None and movie_id > 0)
        limit = parameter(config, "movie_id_range")
        check("movie_id_range", movie_id is not None and limit["min"] <= movie_id <= limit["max"])
        check("movie_genre_domain", all(value in GENRES for value in values))
        check("movie_genre_unique", len(values) == len(set(values)))
        year = re.search(r"\((\d{4})\)$", title)
        check("movie_title_year", year is not None)
        check("movie_title_year_max", year is None or 1 <= int(year.group(1)) <= parameter(config, "movie_title_year_max"))
    else:
        uid, mid, rating, stamp = parts
        check("rating_user_id_positive", (integer(uid) or 0) > 0)
        check("rating_movie_id_positive", (integer(mid) or 0) > 0)
        check("rating_integer", (integer(rating) or 0) > 0)
        try:
            number = float(rating)
            limit = parameter(config, "rating_range")
            in_range = limit["min"] <= number <= limit["max"]
        except ValueError:
            in_range = False
        check("rating_range", in_range)
        timestamp = integer(stamp)
        check("timestamp_integer", timestamp is not None)
        limit = parameter(config, "timestamp_range")
        check("timestamp_range", timestamp is not None and limit["min"] <= timestamp <= limit["max"])
    return found


def key(kind, parts):
    return parts[0] if kind != "2" else (parts[0], parts[1], parts[3])


def rows():
    for line in sys.stdin:
        sort_key, encoded = line.rstrip("\r\n").split("\t", 1)
        yield sort_key[0], json.loads(encoded)


def clean(config):
    seen = {kind: set() for kind in FIELDS}
    canonical = {"0": {}, "1": {}}
    users, movies = set(), set()
    counts = {"raw": Counter(), "clean": Counter(), "removed": Counter(), "normalized": Counter(), "by_rule": Counter()}
    examples = []
    for kind, raw in rows():
        counts["raw"][kind] += 1
        original = raw.split("::")
        parts = [x.strip() for x in original]
        if len(parts) == FIELDS[kind] and all(parts) and kind == "1" and enabled(config, "normalize_genres"):
            parts[2] = "|".join(dict.fromkeys(x.strip() for x in parts[2].split("|")))
        candidates = issues(kind, parts, config, raw)
        active = list(dict.fromkeys(x for x in candidates if enabled(config, x)))
        # Business-key deduplication only considers records that passed parsing and field checks.
        if not active:
            item_key = key(kind, parts)
            dup_rule = {"0": "unique_users", "1": "unique_movies", "2": "unique_ratings"}[kind]
            if item_key in seen[kind]:
                candidates.append(dup_rule)
            prior = canonical.get(kind, {}).get(item_key)
            conflict_rule = "consistent_users" if kind == "0" else "consistent_movies"
            if prior is not None and prior != tuple(parts):
                candidates.append(conflict_rule)
            if kind == "2":
                if parts[0] not in users:
                    candidates.append("rating_user_exists")
                if parts[1] not in movies:
                    candidates.append("rating_movie_exists")
            active = list(dict.fromkeys(x for x in candidates if enabled(config, x)))
        if active:
            counts["removed"][kind] += 1
            for rule in active:
                counts["by_rule"][rule] += 1
            action = {"table": kind, "action": "removed", "rules": active, "line": raw}
            emit("A", action)
            if len(examples) < 20:
                examples.append(action)
            continue
        if len(parts) == FIELDS[kind]:
            item_key = key(kind, parts)
            seen[kind].add(item_key)
            if kind in canonical:
                canonical[kind].setdefault(item_key, tuple(parts))
        if kind == "0":
            users.add(parts[0])
        elif kind == "1":
            movies.add(parts[0])
        output_parts = parts if enabled(config, "trim_fields") else list(original)
        if kind == "1" and enabled(config, "normalize_genres"):
            output_parts[2] = parts[2]
        normalized = "::".join(output_parts)
        counts["clean"][kind] += 1
        emit("C", {"table": kind, "line": normalized})
        if normalized != raw:
            counts["normalized"][kind] += 1
            changes = []
            if original != [x.strip() for x in original] and enabled(config, "trim_fields"):
                changes.append("trim_fields")
            if kind == "1" and parts[2] != original[2]:
                changes.append("normalize_genres")
            emit("A", {"table": kind, "action": "normalized", "rules": changes, "line": raw, "output": normalized})
    emit("M", {"counts": {name: dict(value) for name, value in counts.items()}, "examples": examples,
               "limits": ["Removed rows remain in actions.ndjson; no value imputation is performed."]})


def assess(config):
    dimensions = ("accurate", "complete", "unique", "consistent", "up_to_date")
    tables = {name: {kind: Counter() for kind in FIELDS} for name in dimensions}
    counts = Counter()
    groups = {"0": {}, "1": {}, "2": {}}
    cohort_rows = Counter()
    user_movies = {}
    stamps = []
    window = parameter(config, "score_time_window")
    start, end = window["min"], window["max"]

    def add(name, kind, eligible, good):
        metric = tables[name][kind]
        metric["eligible"] += int(eligible)
        metric["good"] += int(eligible and good)

    def valid_id(value, limit):
        parsed = integer(value)
        return parsed is not None and limit[0] <= parsed <= limit[1]

    for kind, raw in rows():
        counts[kind] += 1
        parts = [part.strip() for part in raw.split("::")]
        basic = len(parts) == FIELDS[kind] and all(parts)
        add("complete", kind, True, basic and kind != "0")
        if not basic:
            continue
        uid = integer(parts[0])
        if kind == "0":
            occupation = integer(parts[3])
            accurate = (valid_id(parts[0], (1, 6040)) and parts[1] in {"F", "M"}
                        and parts[2] in AGES and occupation is not None and 0 <= occupation <= 20)
            item_key = uid if uid is not None and uid > 0 else None
            if item_key is not None:
                cohort_rows[item_key] += 1
        elif kind == "1":
            genres = parts[2].split("|")
            accurate = valid_id(parts[0], (1, 3952)) and all(x in GENRES for x in genres)
            item_key = uid if uid is not None and uid > 0 else None
        else:
            movie_id, rating, stamp = (integer(x) for x in parts[1:])
            accurate = (valid_id(parts[0], (1, 6040)) and valid_id(parts[1], (1, 3952))
                        and rating is not None and 1 <= rating <= 5)
            item_key = (uid, movie_id, stamp) if (uid is not None and uid > 0 and
                        movie_id is not None and movie_id > 0 and stamp is not None) else None
            add("up_to_date", kind, True, stamp is not None and start <= stamp <= end)
            if stamp is not None and start <= stamp <= end:
                stamps.append(stamp)
        add("accurate", kind, True, accurate)
        if item_key is None:
            continue
        add("unique", kind, True, item_key not in groups[kind])
        add("consistent", kind, True, False)
        signature = tuple(parts[1:]) if kind != "2" else rating
        group = groups[kind].setdefault(item_key, [0, signature, False, 0])
        group[0] += 1
        group[2] |= group[1] != signature
        group[3] += int(accurate)

    valid_users = {key for key, group in groups["0"].items()
                   if group[0] == 1 and group[3] == 1 and not group[2]}
    valid_movies = {key for key, group in groups["1"].items()
                    if group[0] == 1 and group[3] == 1 and not group[2]}
    for (uid, mid, stamp), group in groups["2"].items():
        if (uid in valid_users and mid in valid_movies and start <= stamp <= end
                and group[3] == group[0] and not group[2]):
            user_movies.setdefault(uid, set()).add(mid)
    for kind in ("0", "1"):
        tables["consistent"][kind]["good"] = sum(group[0] for group in groups[kind].values() if not group[2])
    tables["consistent"]["2"]["good"] = sum(
        group[0] for (uid, mid, _), group in groups["2"].items()
        if not group[2] and uid in valid_users and mid in valid_movies)
    qualified = {uid for uid in valid_users if len(user_movies.get(uid, ())) >= 20}
    tables["complete"]["0"]["good"] = sum(cohort_rows[uid] for uid in qualified)
    enabled_scores = {name: enabled(config, "score_" + name) for name in dimensions}
    result = {}
    for name in dimensions:
        by_table = {}
        applicable = ("2",) if name == "up_to_date" else tuple(FIELDS)
        scores, coverages = [], []
        for kind in applicable:
            metric = tables[name][kind]
            total = counts[kind]
            eligible = metric["eligible"]
            good = metric["good"]
            value = good / eligible * 100 if eligible else None
            coverage = eligible / total * 100 if total else None
            if value is not None:
                scores.append(value)
            if coverage is not None:
                coverages.append(coverage)
            by_table[kind] = {"good": good, "eligible": eligible, "total": total,
                              "bad": eligible - good, "unassessable": total - eligible,
                              "score": value, "coverage": coverage}
        result[name] = {"score": sum(scores) / len(scores) if scores and enabled_scores[name] else None,
                        "coverage": sum(coverages) / len(coverages) if coverages else None,
                        "tables": by_table}
    stamps.sort()
    t1 = stamps[int((len(stamps) - 1) * .70)] if stamps else None
    t2 = stamps[int((len(stamps) - 1) * .85)] if stamps else None
    emit("M", {"score_spec_version": "quality-spec-v2", "counts": dict(counts),
               "dimensions": result, "enabled": enabled_scores, "time_window": window,
               "cohort20": {"qualified_users": len(qualified), "evaluable_users": len(valid_users),
                            "rate": len(qualified) / len(valid_users) * 100 if valid_users else None,
                            "under_threshold_users": len(valid_users) - len(qualified)},
               "t1": t1, "t2": t2,
               "limits": ["20-film cohort is a completeness diagnostic; cleaning never removes users for this criterion.",
                          "Accuracy checks known domains, not real-world truth; T1/T2 are descriptive."]})


def main():
    if len(sys.argv) != 2 or sys.argv[1] not in {"map", "clean", "assess"}:
        raise SystemExit("usage: govern.py map|clean|assess")
    if sys.argv[1] == "map":
        mapper()
    else:
        with open("rules.json", encoding="utf-8") as file:
            config = json.load(file)
        (clean if sys.argv[1] == "clean" else assess)(config)

if __name__ == "__main__":
    main()
