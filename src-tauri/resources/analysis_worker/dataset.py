import hashlib
import shutil
from collections import Counter
from pathlib import Path
import numpy as np
from .common import fail, read_json, write_json, progress, rows_file

GENRES = ['Action','Adventure','Animation',"Children's",'Comedy','Crime','Documentary','Drama','Fantasy','Film-Noir','Horror','Musical','Mystery','Romance','Sci-Fi','Thriller','War','Western']


def prepare(request):
    info = request['dataset']
    t1, t2 = info.get('t1'), info.get('t2')
    if not isinstance(t1, int) or not isinstance(t2, int) or not 0 <= t1 < t2:
        fail('invalid_time_boundaries', '清洗后评估缺少有效的训练、验证截止时间')
    root = Path(request['dataset_dir'])
    root.mkdir(parents=True, exist_ok=True)
    if (root / 'manifest.json').is_file():
        manifest = read_json(root / 'manifest.json')
        if any(manifest.get(k) != info.get(k) for k in ['id','clean_version','rule_version','t1','t2']):
            fail('version_mismatch', '数据版本或时间切分与已登记数据集不一致')
        return manifest
    progress('校验清洗版本与分析记录')
    digest = hashlib.sha256()
    for name in ['users.dat','movies.dat','ratings.dat']:
        source = Path(info['files'][name])
        if not source.is_file():
            fail('data_unavailable', f'清洗文件 {name} 不存在')
        digest.update(name.encode())
        with source.open('rb') as f:
            for chunk in iter(lambda: f.read(65536), b''):
                digest.update(chunk)
        if source.resolve() != (root / name).resolve():
            shutil.copyfile(source, root / name)
    if info['clean_version'] != 'sha256:' + digest.hexdigest():
        fail('version_mismatch', '清洗文件校验和与登记版本不一致')
    rejected = []
    def reject(table, line, reason):
        rejected.append({'table': table, 'line': line, 'reason': reason})
    def entities(name, fields):
        records, conflicts = {}, set()
        for line, raw in enumerate((root / name).read_text(encoding='utf-8').splitlines(), 1):
            parts = [x.strip() for x in raw.split('::')]
            try:
                if len(parts) != fields or not all(parts) or not parts[0].isdigit() or int(parts[0]) <= 0:
                    raise ValueError()
                key = int(parts[0])
                if name == 'movies.dat' and not all(g in GENRES for g in parts[2].split('|')):
                    raise ValueError()
                if key in records:
                    if records[key] != parts:
                        conflicts.add(key)
                    reject(name, line, '重复或冲突的实体 ID')
                else:
                    records[key] = parts
            except (ValueError, IndexError):
                reject(name, line, '结构或字段不适合建模')
        for key in conflicts:
            records.pop(key, None)
        return records
    users = entities('users.dat', 5)
    movies = entities('movies.dat', 3)
    events, conflicts = {}, set()
    with (root / 'ratings.dat').open(encoding='utf-8') as f:
        for line, raw in enumerate(f, 1):
            p = raw.strip().split('::')
            try:
                if len(p) != 4 or any(not x.isdigit() for x in p):
                    raise ValueError()
                uid, mid, rating, stamp = map(int, p)
                if uid not in users or mid not in movies or rating not in range(1,6) or stamp < 0:
                    raise ValueError()
                event = (uid, mid, stamp)
                if event in events:
                    if events[event] != rating:
                        conflicts.add(event)
                    reject('ratings.dat', line, '重复或冲突评分事件')
                else:
                    events[event] = rating
            except (ValueError, OverflowError):
                reject('ratings.dat', line, '评分、时间或跨表引用无效')
    data = np.asarray([[u,m,r,t] for (u,m,t),r in events.items() if (u,m,t) not in conflicts], dtype=np.int64)
    if data.size == 0:
        fail('data_unavailable', '没有适合分析的评分事件')
    data = data[np.argsort(data[:,3], kind='stable')]
    counts = {'train': int(np.sum(data[:,3] <= t1)), 'validation': int(np.sum((data[:,3]>t1)&(data[:,3]<=t2))), 'test': int(np.sum(data[:,3] > t2))}
    if counts['train'] == 0:
        fail('data_unavailable', '训练期没有评分事件')
    np.save(root / 'ratings.npy', data)
    write_json(root / 'entities.json', {'users': users, 'movies': movies})
    rows_file(root, 'excluded', rejected)
    manifest = {k:v for k,v in info.items() if k != 'files'}
    manifest.update(split_counts=counts, excluded_counts=dict(Counter(x['reason'] for x in rejected)), feature_version='causal-v1')
    write_json(root / 'manifest.json', manifest)
    return manifest


def load(root):
    root = Path(root)
    if not (root / 'manifest.json').is_file():
        fail('data_unavailable', '分析数据集尚未登记')
    manifest = read_json(root / 'manifest.json')
    entities = read_json(root / 'entities.json')
    return manifest, np.load(root / 'ratings.npy', mmap_mode='r'), entities
