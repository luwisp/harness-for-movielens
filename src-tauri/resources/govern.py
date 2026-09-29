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


def issues(kind, parts, config):
    if len(parts) != FIELDS[kind]:
        return ["schema_fields"]
    if not all(item.strip() for item in parts):
        return ["required_values"]
    found = []
    def check(rule, good):
        if not good:
            found.append(rule)
    if kind == "0":
        uid, gender, age, occupation, zipcode = parts
        check("user_id_positive", (integer(uid) or 0) > 0)
        check("user_gender_domain", gender in {"F", "M"})
        check("user_age_domain", age in AGES)
        limit = parameter(config, "user_occupation_range")
        check("user_occupation_range", integer(occupation) is not None and limit["min"] <= int(occupation) <= limit["max"])
        check("user_zip_format", re.fullmatch(r"\d{5}(?:-\d{4})?", zipcode) is not None)
    elif kind == "1":
        mid, title, genres = parts
        values = genres.split("|")
        check("movie_id_positive", (integer(mid) or 0) > 0)
        check("movie_genre_domain", all(value in GENRES for value in values))
        check("movie_genre_unique", len(values) == len(set(values)))
        check("movie_title_year", re.search(r"\(\d{4}\)$", title) is not None)
    else:
        uid, mid, rating, stamp = parts
        check("rating_user_id_positive", (integer(uid) or 0) > 0)
        check("rating_movie_id_positive", (integer(mid) or 0) > 0)
        check("rating_integer", integer(rating) is not None)
        try:
            number = float(rating)
            limit = parameter(config, "rating_range")
            in_range = limit["min"] <= number <= limit["max"]
        except ValueError:
            in_range = False
        check("rating_range", in_range)
        check("timestamp_integer", integer(stamp) is not None)
        limit = parameter(config, "timestamp_range")
        check("timestamp_range", integer(stamp) is not None and limit["min"] <= int(stamp) <= limit["max"])
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
        candidates = issues(kind, parts, config)
        if len(parts) == FIELDS[kind] and all(parts):
            if kind == "1" and enabled(config, "normalize_genres"):
                genres = "|".join(dict.fromkeys(x.strip() for x in parts[2].split("|")))
                if genres != parts[2]:
                    parts[2] = genres
                    candidates = issues(kind, parts, config)
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
    counts = Counter()
    score = Counter()
    seen = {kind: set() for kind in FIELDS}
    canonical = {"0": {}, "1": {}}
    users, movies = set(), set()
    stamps = []
    valid_stamps = []
    for kind, raw in rows():
        counts[kind] += 1
        parts = [x.strip() for x in raw.split("::")]
        basic = len(parts) == FIELDS[kind] and all(parts)
        found = issues(kind, parts, config)
        score["complete_eligible"] += 1
        score["complete_good"] += int(basic)
        score["accurate_eligible"] += 1
        accuracy_rules = {"user_id_positive", "user_gender_domain", "user_age_domain", "user_occupation_range",
                          "movie_id_positive", "movie_genre_domain", "rating_user_id_positive",
                          "rating_movie_id_positive", "rating_integer", "rating_range", "timestamp_integer", "timestamp_range"}
        score["accurate_good"] += int(basic and not any(x in accuracy_rules for x in found))
        score["unique_eligible"] += 1
        score["consistent_eligible"] += 1
        unique = False
        consistent = False
        if basic:
            item_key = key(kind, parts)
            unique = item_key not in seen[kind]
            seen[kind].add(item_key)
            if kind == "0":
                consistent = item_key not in canonical[kind] or canonical[kind][item_key] == tuple(parts)
                canonical[kind].setdefault(item_key, tuple(parts))
                users.add(item_key)
            elif kind == "1":
                values = parts[2].split("|")
                consistent = len(values) == len(set(values)) and (item_key not in canonical[kind] or canonical[kind][item_key] == tuple(parts))
                canonical[kind].setdefault(item_key, tuple(parts))
                movies.add(item_key)
            else:
                consistent = parts[0] in users and parts[1] in movies
                stamp = integer(parts[3])
                if stamp is not None:
                    stamps.append(stamp)
                    limits = parameter(config, "timestamp_range")
                    if limits["min"] <= stamp <= limits["max"]:
                        valid_stamps.append(stamp)
        score["unique_good"] += int(unique)
        score["consistent_good"] += int(consistent)
        if kind == "2":
            score["up_to_date_eligible"] += 1
    reference = config.get("assessment_reference") or (parameter(config, "score_reference") if enabled(config, "score_reference") else 0) or (max(valid_stamps) if valid_stamps else None)
    if reference is not None:
        low = reference - parameter(config, "score_freshness") * 86400
        score["up_to_date_good"] = sum(low <= stamp <= reference for stamp in stamps)
    enabled_scores = {name: enabled(config, "score_" + name) for name in ("accurate", "complete", "unique", "consistent")}
    enabled_scores["up_to_date"] = enabled(config, "score_freshness")
    valid_stamps.sort()
    t1 = valid_stamps[int((len(valid_stamps) - 1) * .70)] if valid_stamps else None
    t2 = valid_stamps[int((len(valid_stamps) - 1) * .85)] if valid_stamps else None
    emit("M", {"counts": dict(counts), "scores": dict(score), "enabled": enabled_scores,
               "reference_timestamp": reference, "t1": t1, "t2": t2,
               "limits": ["Accuracy checks known formats and domains, not real-world truth.",
                          "Freshness uses a historical reference; T1/T2 remain descriptive quantiles."]})


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
