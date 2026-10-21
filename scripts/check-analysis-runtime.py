"""Exercise the built worker through its public JSONL protocol, using synthetic fixtures."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def check(executable):
    with tempfile.TemporaryDirectory(prefix='ml-worker-check-') as temp:
        root=Path(temp);source=root/'input';source.mkdir()
        (source/'users.dat').write_text('\n'.join(f'{u}::M::25::1::00123' for u in range(1,17)),encoding='utf-8')
        genres=['Action','Comedy','Drama','Romance','Sci-Fi','War']
        (source/'movies.dat').write_text('\n'.join(f'{m}::Film {m} (2000)::{g}' for m,g in enumerate(genres,1)),encoding='utf-8')
        (source/'ratings.dat').write_text('\n'.join(f'{t%16+1}::{(t//16+t%16+1)%6+1}::{5 if (t+t//16)%3 else 2}::{t}' for t in range(1,481)),encoding='utf-8')
        digest=hashlib.sha256()
        names=['users.dat','movies.dat','ratings.dat']
        for name in names:digest.update(name.encode());digest.update((source/name).read_bytes())
        base={'dataset_dir':str(root/'dataset'),'assets_dir':str(root/'assets')}
        info={'id':'fixture','label':'自动化测试数据','rule_label':'测试规则','rule_version':'fixture','clean_version':'sha256:'+digest.hexdigest(),'t1':300,'t2':390,'files':{n:str(source/n) for n in names}}
        request=root/'request.json'
        def run(config):
            request.write_text(json.dumps({**base,**config}),encoding='utf-8')
            # Match the real Rust/SSH parent: keep stdin open until the child exits.
            # File-backed output avoids pipe backpressure while waiting for shutdown.
            with tempfile.TemporaryFile() as output, tempfile.TemporaryFile() as errors:
                with subprocess.Popen([str(executable),str(request)],stdin=subprocess.PIPE,
                        stdout=output,stderr=errors,env={**os.environ,'ML_WATCH_STDIN':'1',
                        'PYTHONUTF8':'1','OMP_NUM_THREADS':'2','OPENBLAS_NUM_THREADS':'2'}) as process:
                    try:
                        code=process.wait(timeout=60)
                    except subprocess.TimeoutExpired:
                        process.kill();process.wait();raise
                output.seek(0);stdout=output.read().decode('utf-8')
                errors.seek(0);stderr=errors.read().decode('utf-8')
            events=[json.loads(line) for line in stdout.splitlines()]
            assert code==0,(stdout,stderr)
            assert 'Fatal Python error' not in stderr,stderr
            assert events[-1]['event']=='result',events
            return events[-1]['result']
        run({'action':'prepare','dataset':info})
        for action in ['logistic','tree','kmeans','dbscan','svd','cur']:
            parameters={'seed':42}
            if action in ['kmeans','dbscan','svd','cur']:parameters['min_history']=1
            result=run({'action':action,'parameters':parameters,**({'user_id':1,'movie_id':1} if action in ['logistic','tree'] else {})})
            assert result['new_asset'] and result['reportable']
            assert (root/'assets'/result['asset_id']/'metadata.json').is_file()
            history=run({'action':action,'model_id':result['asset_id'],'mode':'lookup'})
            assert not history['new_asset'] and not history['reportable']
            print(action+' packaged worker OK',flush=True)
        # Closing the parent's pipe must terminate the worker even during native computation.
        request.write_text(json.dumps({**base,'action':'check'}),encoding='utf-8')
        process=subprocess.Popen([str(executable),str(request)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env={**os.environ,'ML_WATCH_STDIN':'1','PYTHONUTF8':'1'})
        process.stdin.close()
        try:process.wait(timeout=20)
        except subprocess.TimeoutExpired:
            process.kill();process.wait();raise AssertionError('parent disconnect did not stop worker')
        assert process.returncode in [0,130],process.returncode

if __name__=='__main__':check(Path(sys.argv[1]).resolve())
