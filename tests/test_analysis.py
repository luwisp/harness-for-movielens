import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path
import numpy as np

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'src-tauri'/'resources'))
from analysis_worker.dataset import prepare,load
from analysis_worker.features import FeatureBuilder
from analysis_worker.common import Assets,AnalysisError
from analysis_worker.classification import run as classify
from analysis_worker.clustering import run as clustering,metrics
from analysis_worker.reduction import cur_reduce,svd_reduce,reconstruction,run as reduction
from analysis_worker.profiles import build

class AnalysisTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(); self.root=Path(self.temp.name)
        source=self.root/'input'; source.mkdir()
        (source/'users.dat').write_text('\n'.join(f'{u}::M::25::1::00123' for u in range(1,17)))
        genres=['Action','Comedy','Drama','Romance','Sci-Fi','War']
        (source/'movies.dat').write_text('\n'.join(f'{m}::Film {m} (2000)::{g}' for m,g in enumerate(genres,1)))
        events=[]
        for t in range(1,481):
            u=t%16+1; m=(t//16+u)%6+1; r=5 if (m+u)%3 else 2
            events.append(f'{u}::{m}::{r}::{t}')
        (source/'ratings.dat').write_text('\n'.join(events))
        digest=hashlib.sha256()
        for name in ['users.dat','movies.dat','ratings.dat']:
            digest.update(name.encode());digest.update((source/name).read_bytes())
        self.request={'dataset_dir':str(self.root/'dataset'),'assets_dir':str(self.root/'assets'),
            'dataset':{'id':'dataset-test','label':'测试清洗版本','rule_label':'测试规则','rule_version':'rules-test',
            'clean_version':'sha256:'+digest.hexdigest(),'t1':300,'t2':390,'files':{n:str(source/n) for n in ['users.dat','movies.dat','ratings.dat']}}}
        prepare(self.request)
        self.dataset,self.data,self.entities=load(self.request['dataset_dir'])
        self.assets=Assets(self.request['assets_dir'])
    def tearDown(self): self.temp.cleanup()
    def test_causal_prefix_and_frozen_future(self):
        rows=np.array([[1,1,5,1],[1,2,1,1],[1,1,2,2],[1,2,5,4]])
        a=FeatureBuilder().fit_transform(rows,self.entities,2)
        changed=rows.copy();changed[-1,2]=1
        b=FeatureBuilder().fit_transform(changed,self.entities,2)
        np.testing.assert_array_equal(a,b)
        self.assertAlmostEqual(a[0,-4],3.)
        self.assertAlmostEqual(a[1,-4],3.)
        self.assertEqual(a[2,-3],np.float32(np.log1p(2)))
    def test_version_mismatch_rejected(self):
        req={**self.request,'dataset':{**self.request['dataset'],'t1':299}}
        with self.assertRaises(AnalysisError): prepare(req)
    def test_both_classifiers_and_unknown_id(self):
        for algo in ['logistic','tree']:
            req={**self.request,'user_id':1,'movie_id':1,'parameters':({'C':1} if algo=='logistic' else {'max_depth':4,'min_samples_leaf':2})}
            result,folder=classify(req,self.dataset,self.data,self.entities,self.assets,algo)
            self.assertTrue(0<=result['target']['probability']<=1)
            self.assertIn('test',result['metrics'])
            self.assertTrue((folder/'model.pkl').exists())
            bad={**req,'user_id':999,'model_id':result['asset_id']}
            with self.assertRaises(AnalysisError) as err: classify(bad,self.dataset,self.data,self.entities,self.assets,algo)
            self.assertEqual(err.exception.code,'unknown_user')
    def test_profiles_ignore_future_ratings(self):
        req={**self.request,'parameters':{'min_history':1}}
        u,x,_,_=build(self.data,self.entities,300,req)
        changed=np.array(self.data); changed[changed[:,3]>300,2]=1
        uu,xx,_,_=build(changed,self.entities,300,req)
        np.testing.assert_array_equal(u,uu);np.testing.assert_array_equal(x,xx)
    def test_cluster_algorithms_and_na_metrics(self):
        for algo,params in [('kmeans',{'n_clusters':2,'min_history':1}),('dbscan',{'eps':.5,'min_samples':2,'min_history':1})]:
            r,_=clustering({**self.request,'parameters':params,'user_id':1},self.dataset,self.data,self.entities,self.assets,algo)
            self.assertEqual(r['memberships'][0]['user_id'],1)
            self.assertIn('noise_rate',r['metrics'])
        m,note=metrics(np.eye(4),np.array([-1,-1,-1,-1]))
        self.assertIsNone(m['silhouette']);self.assertTrue(note)
    def test_cur_shape_and_reproducibility(self):
        x=np.random.default_rng(7).random((20,6))
        z,a,m=cur_reduce(x,3,seed=42)
        zz,aa,mm=cur_reduce(x,3,seed=42)
        self.assertEqual(z.shape,(20,3));self.assertEqual(a.shape,x.shape)
        self.assertEqual(len(set(m['columns'])),3)
        np.testing.assert_array_equal(z,zz);np.testing.assert_array_equal(a,aa)
        self.assertLessEqual(reconstruction(x,a)['relative_error'],1+1e-10)
        _,full,_=svd_reduce(x,6)
        self.assertLess(reconstruction(x,full)['relative_error'],1e-20)
    def test_both_reductions_have_real_comparison_and_plots(self):
        for algo in ['svd','cur']:
            req={**self.request,'parameters':{'dimensions':2,'min_history':1},'clustering_parameters':{'n_clusters':2}}
            r,folder=reduction(req,self.dataset,self.data,self.entities,self.assets,algo)
            self.assertEqual(r['dimensions'],2);self.assertEqual(len(r['comparison']),2)
            self.assertTrue((folder/'projection.svg').is_file())
            self.assertTrue((folder/'coordinates.jsonl').is_file())
            self.assertTrue(np.isfinite(r['information']['relative_error']))
    def test_lookup_missing_and_invalid_dimensions(self):
        with self.assertRaises(AnalysisError):
            reduction({**self.request,'parameters':{'dimensions':100}},self.dataset,self.data,self.entities,self.assets,'cur')
        with self.assertRaises(AnalysisError) as err:
            classify({**self.request,'mode':'predict'},self.dataset,self.data,self.entities,self.assets,'logistic')
        self.assertEqual(err.exception.code,'model_unavailable')


class ProtocolTests(unittest.TestCase):
    setUp=AnalysisTests.setUp
    tearDown=AnalysisTests.tearDown
    def test_worker_protocol_and_history_no_report(self):
        import os,subprocess
        env={**os.environ,'PYTHONPATH':str(Path(__file__).resolve().parents[1]/'src-tauri/resources'),'PYTHONDONTWRITEBYTECODE':'1','OPENBLAS_NUM_THREADS':'2'}
        request=Path(self.root/'request.json')
        def run(req):
            request.write_text(json.dumps(req))
            p=subprocess.run([sys.executable,'-m','analysis_worker',str(request)],text=True,capture_output=True,env=env,timeout=30)
            events=[json.loads(line) for line in p.stdout.splitlines()]
            return p,events[-1]
        p,event=run({**self.request,'action':'tree','parameters':{'max_depth':3,'min_samples_leaf':2}})
        self.assertEqual(p.returncode,0,p.stderr);result=event['result'];self.assertTrue(result['new_asset']);self.assertTrue(result['reportable'])
        p,event=run({**self.request,'action':'tree','model_id':result['asset_id'],'mode':'lookup','user_id':1,'movie_id':1})
        self.assertEqual(p.returncode,0,p.stderr);self.assertFalse(event['result']['new_asset']);self.assertFalse(event['result']['reportable'])
        p,event=run({**self.request,'action':'tree','user_id':999,'movie_id':1})
        self.assertEqual(p.returncode,2);self.assertEqual(event['code'],'unknown_user')
    def test_explicit_group_model_keeps_original_history_threshold(self):
        short=np.asarray(self.data[:48])
        req={**self.request,'parameters':{'min_history':1,'n_clusters':2}}
        result,_=clustering(req,self.dataset,short,self.entities,self.assets,'kmeans')
        reused,_=clustering({**self.request,'model_id':result['asset_id'],'mode':'lookup'},self.dataset,short,self.entities,self.assets,'kmeans')
        self.assertEqual(result['asset_id'],reused['asset_id'])
    def test_single_class_auc_unavailable(self):
        from analysis_worker.classification import scores
        met,note=scores(np.ones(3),np.array([.3,.6,.9]),.5)
        self.assertIsNone(met['roc_auc']);self.assertIsNone(met['pr_auc']);self.assertTrue(note)
    def test_invalid_target_does_not_publish(self):
        with self.assertRaises(AnalysisError):clustering({**self.request,'user_id':999},self.dataset,self.data,self.entities,self.assets,'kmeans')
        self.assertEqual(list(self.assets.root.glob('*/metadata.json')),[])
    def test_validation_changes_do_not_change_profiles(self):
        users,x,_,_=build(self.data,self.entities,300,{'parameters':{'min_history':1}})
        changed=np.array(self.data);changed[changed[:,3]>300,0]=1
        uu,xx,_,_=build(changed,self.entities,300,{'parameters':{'min_history':1}})
        np.testing.assert_array_equal(users,uu);np.testing.assert_array_equal(x,xx)

if __name__=='__main__': unittest.main()
