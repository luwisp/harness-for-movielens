import csv
import hashlib
import json
import os
import pickle
import time
import uuid
from pathlib import Path
import numpy as np

class AnalysisError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


def fail(code, message):
    raise AnalysisError(code, message)


def progress(stage):
    print(json.dumps({'event': 'progress', 'stage': stage}, ensure_ascii=False), flush=True)


def plain(value):
    if isinstance(value, dict):
        return {str(k): plain(v) for k, v in value.items()}
    if isinstance(value, (list, tuple)):
        return [plain(x) for x in value]
    if isinstance(value, np.ndarray):
        return plain(value.tolist())
    if isinstance(value, np.generic):
        return plain(value.item())
    if isinstance(value, float) and not np.isfinite(value):
        return None
    return value


def write_json(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temp = path.with_name(path.name + '.tmp')
    temp.write_text(json.dumps(plain(value), ensure_ascii=False, allow_nan=False), encoding='utf-8')
    os.replace(temp, path)


def read_json(path):
    return json.loads(Path(path).read_text(encoding='utf-8'))


def rows_file(folder, name, rows):
    rows = list(rows)
    with (Path(folder) / (name + '.jsonl')).open('w', encoding='utf-8') as f:
        for row in rows:
            f.write(json.dumps(plain(row), ensure_ascii=False, allow_nan=False) + '\n')
    if rows:
        with (Path(folder) / (name + '.csv')).open('w', encoding='utf-8-sig', newline='') as f:
            writer = csv.DictWriter(f, fieldnames=list(rows[0]))
            writer.writeheader()
            for row in rows:
                writer.writerow({k: json.dumps(plain(v), ensure_ascii=False) if isinstance(v, (list, dict)) else v for k, v in row.items()})
    return rows


def number(params, key, default, low, high, integer=False):
    value = params.get(key, default)
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not np.isfinite(value):
        fail('invalid_parameters', f'{key} 必须为有效数值')
    if integer and int(value) != value:
        fail('invalid_parameters', f'{key} 必须为整数')
    if not low <= value <= high:
        fail('invalid_parameters', f'{key} 必须在 {low} 至 {high} 之间')
    return int(value) if integer else float(value)


def key_for(task, dataset, params, cohort=None):
    body = {'task': task, 'dataset': dataset, 'params': params, 'cohort': cohort, 'version': 'analysis-v1'}
    return hashlib.sha256(json.dumps(body, sort_keys=True).encode()).hexdigest()


class Assets:
    def __init__(self, root):
        self.root = Path(root)
        self.root.mkdir(parents=True, exist_ok=True)

    def find(self, key, asset_id=None):
        folders = [self.root / str(uuid.UUID(asset_id))] if asset_id else self.root.iterdir()
        for folder in folders:
            meta = folder / 'metadata.json'
            if not meta.is_file() or folder.name.startswith('.'):
                continue
            data = read_json(meta)
            if (folder / 'result.json').is_file() and (folder / 'model.pkl').is_file() and (asset_id or data.get('cache_key') == key):
                return folder, data
        return None

    def create(self, task, key, dataset, params, label):
        asset_id = str(uuid.uuid4())
        folder = self.root / ('.tmp-' + asset_id)
        folder.mkdir()
        ordinal = 1 + sum(1 for p in self.root.glob('*/metadata.json') if read_json(p).get('task') == task)
        meta = {'id': asset_id, 'task': task, 'cache_key': key, 'dataset_id': dataset['id'],
                'version': f'{label} v{ordinal}', 'parameters': params, 'created_at': time.time()}
        return folder, meta

    def publish(self, folder, meta, result, bundle=None):
        if bundle is not None:
            with (folder / 'model.pkl').open('wb') as f:
                pickle.dump(bundle, f, protocol=5)
        result.update(asset_id=meta['id'], version=meta['version'], parameters=meta['parameters'])
        write_json(folder / 'result.json', result)
        write_json(folder / 'metadata.json', meta)
        final = self.root / meta['id']
        os.replace(folder, final)
        return final, meta


def base_result(dataset, task, algorithm):
    limitations=[]
    excluded=dataset.get('excluded_counts',{})
    if excluded: limitations.append('建模视图排除不适合分析的记录：'+'；'.join(f'{k} {v} 条' for k,v in excluded.items())+'。不改写清洗文件。')
    return {'task': task, 'algorithm': algorithm, 'data_version': dataset['label'],
            'rule_version': dataset['rule_label'], 'dataset_id': dataset['id'],
            'split': {'train_end': dataset['t1'], 'validation_end': dataset['t2'], 'counts': dataset['split_counts']},
            'limitations': limitations}
