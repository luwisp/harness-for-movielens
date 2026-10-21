import json
import os
import sys
import threading
import time
from pathlib import Path



def watch_parent():
    # Use an independent raw descriptor: a daemon blocked in stdin.buffer.read()
    # holds BufferedReader's lock and can abort Python during normal shutdown.
    descriptor = os.dup(sys.stdin.fileno())

    def watch():
        while True:
            try:
                if not os.read(descriptor, 4096):
                    os._exit(130)
            except InterruptedError:
                continue
            except OSError:
                os._exit(130)

    thread = threading.Thread(target=watch, name='analysis-parent-watch', daemon=True)
    thread.start()
    return thread


def main():
    from .common import AnalysisError,Assets,read_json,write_json,plain
    from . import dataset,classification,clustering,reduction
    import numpy,scipy,sklearn,psutil
    if len(sys.argv)==2 and sys.argv[1]=='--check':
        print(json.dumps({'ok':True,'numpy':numpy.__version__,'scipy':scipy.__version__,'sklearn':sklearn.__version__}))
        return
    request=read_json(sys.argv[1])
    if os.environ.get('ML_WATCH_STDIN')=='1':
        watch_parent()
    memory=[0]; stop=threading.Event()
    def measure():
        process=psutil.Process()
        while not stop.is_set():
            memory[0]=max(memory[0],process.memory_info().rss)
            stop.wait(.05)
    threading.Thread(target=measure,daemon=True).start()
    try:
        action=request['action']
        if action=='check':
            print(json.dumps({'event':'result','result':{'ok':True,'numpy':numpy.__version__,'scipy':scipy.__version__,'sklearn':sklearn.__version__}}),flush=True)
            return
        if action=='prepare':
            result=dataset.prepare(request)
            print(json.dumps({'event':'result','result':plain(result)},ensure_ascii=False,allow_nan=False),flush=True)
            return
        info,data,entities=dataset.load(request['dataset_dir'])
        assets=Assets(request['assets_dir'])
        before={p.name for p in assets.root.iterdir() if not p.name.startswith('.')}
        if action in ['logistic','tree']: result,folder=classification.run(request,info,data,entities,assets,action)
        elif action in ['kmeans','dbscan']: result,folder=clustering.run(request,info,data,entities,assets,action)
        elif action in ['svd','cur']: result,folder=reduction.run(request,info,data,entities,assets,action)
        else: raise AnalysisError('invalid_input','未知分析算法')
        result['new_asset']=folder.name not in before
        result['reportable']=result['new_asset'] or (result['task']=='classification' and 'target' in result and request.get('mode')!='lookup')
        result['dependencies']={'numpy':numpy.__version__,'scipy':scipy.__version__,'sklearn':sklearn.__version__}
        if result['new_asset']:
            stored=read_json(folder/'result.json')
            stored['dependencies']=result['dependencies']
            if 'resources' in result:
                result['resources']['peak_rss_bytes']=memory[0]
                stored['resources']=result['resources']
            write_json(folder/'result.json',stored)
        print(json.dumps({'event':'result','result':plain(result)},ensure_ascii=False,allow_nan=False),flush=True)
    except AnalysisError as e:
        print(json.dumps({'event':'error','code':e.code,'message':str(e)},ensure_ascii=False),flush=True)
        sys.exit(2)
    finally: stop.set()

def entrypoint():
    try: main()
    except (ValueError,KeyError,TypeError,OSError,ImportError) as e:
        print(json.dumps({'event':'error','code':'invalid_input','message':str(e)},ensure_ascii=False),flush=True)
        sys.exit(2)

if __name__=='__main__': entrypoint()
