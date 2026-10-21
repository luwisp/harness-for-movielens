import pickle
import numpy as np
from sklearn.cluster import KMeans,DBSCAN
from sklearn.metrics import silhouette_score,davies_bouldin_score
from .common import fail,number,key_for,base_result,read_json,rows_file,progress
from .profiles import build,summaries,membership


def cluster(x,algorithm,params):
    seed=number(params,'seed',42,0,2147483647,True)
    if algorithm=='kmeans':
        k=number(params,'n_clusters',min(5,len(x)-1),2, min(30,len(x)-1),True)
        n_init=number(params,'n_init',10,1,50,True)
        model=KMeans(n_clusters=k,n_init=n_init,random_state=seed)
        actual={'n_clusters':k,'n_init':n_init,'seed':seed}
    elif algorithm=='dbscan':
        eps=number(params,'eps',.15,.00001,2)
        n=number(params,'min_samples',min(5,len(x)),2,min(10000,len(x)),True)
        model=DBSCAN(eps=eps,min_samples=n,n_jobs=1)
        actual={'eps':eps,'min_samples':n,'seed':seed}
    else: fail('invalid_parameters','不支持的聚类算法')
    return model,model.fit_predict(x),actual


def metrics(x,labels,seed=42):
    keep=labels>=0; groups=len(set(labels[keep])); count=int(keep.sum())
    value={'clusters':groups,'coverage':float(keep.mean()),'noise_rate':float((labels<0).mean()),'silhouette':None,'davies_bouldin':None}
    reason=None
    if groups>=2 and count>groups:
        xx=x[keep]; yy=labels[keep]
        try:
            rng=np.random.default_rng(seed)
            sample=np.sort(rng.choice(count,min(2000,count),replace=False))
            if len(set(yy[sample]))>=2 and len(set(yy[sample]))<len(sample):
                value['silhouette']=float(silhouette_score(xx[sample],yy[sample]))
            value['davies_bouldin']=float(davies_bouldin_score(xx,yy))
            value['silhouette_samples']=len(sample)
        except ValueError: reason='有效群体或样本不足，部分指标不适用'
    else: reason='只有一个群体、全为噪声或有效样本不足，内部指标不适用'
    return value,reason


def period_loss(users,x,labels,data,entities,start,end):
    # Frozen training centroids describe later high-score genre events.
    lookup={int(u):int(g) for u,g in zip(users,labels)}
    fallback=x.mean(axis=0); centers={int(g):x[labels==g].mean(axis=0) for g in set(labels) if g>=0}
    from .dataset import GENRES
    movie_cost={}
    for group,center in {**centers,-1:fallback}.items():
        distribution=(center+.01)/(center.sum()+.01*x.shape[1])
        movie_cost[group]={int(m):float(-np.mean([np.log(distribution[GENRES.index(g)]) for g in v[2].split('|')])) for m,v in entities['movies'].items()}
    total=0.;count=0;assigned=0
    for u,m,r,t in data:
        if start<t<=end and r>=4 and int(u) in lookup:
            group=lookup[int(u)];total+=movie_cost.get(group,movie_cost[-1])[int(m)];count+=1;assigned+=group>=0
    return {'preference_loss':total/count if count else None,'evaluated_events':count,'event_coverage':assigned/count if count else None}


def select_cluster(x,users,data,entities,dataset,algorithm,params):
    from itertools import product
    if algorithm=='kmeans':
        ks=[params['n_clusters']] if 'n_clusters' in params else sorted(set(k for k in [3,5,8] if k<len(x))) or [2]
        configs=[{**params,'n_clusters':k} for k in ks]
    else:
        eps=[params['eps']] if 'eps' in params else [.1,.15,.25]
        sizes=[params['min_samples']] if 'min_samples' in params else sorted(set([min(5,len(x)),min(10,len(x))]))
        configs=[{**params,'eps':e,'min_samples':n} for e,n in product(eps,sizes)]
    best=None
    for i,cfg in enumerate(configs):
        progress(f'训练期拟合与验证期选择：方案 {i+1}/{len(configs)}')
        model,labels,actual=cluster(x,algorithm,cfg)
        validation=period_loss(users,x,labels,data,entities,dataset['t1'],dataset['t2'])
        score=validation['preference_loss']
        # Low coverage cannot win merely by ignoring difficult validation users.
        rank=(score if score is not None else float('inf'),-float(np.mean(labels>=0)))
        if best is None or rank<best[0]:best=(rank,model,labels,actual,validation)
    _,model,labels,actual,validation=best
    test=period_loss(users,x,labels,data,entities,dataset['t2'],np.iinfo(np.int64).max)
    return model,labels,actual,{'validation':validation,'test':test,'selection':'验证期高分类型交叉熵越低越好；噪声采用训练期整体偏好作为回退，不忽略噪声用户。'}


def run(request,dataset,data,entities,assets,algorithm):
    mode=request.get('mode','fit')
    if mode not in ['fit','predict','lookup']: fail('invalid_parameters','运行模式无效')
    if request.get('model_id') and (request.get('parameters') or request.get('user_ids')):
        fail('invalid_parameters','指定已有模型时不能同时更改参数或用户集合，请单独训练新模型')
    params=request.get('parameters') or {}
    allowed={'min_history','seed','n_clusters','n_init'} if algorithm=='kmeans' else {'min_history','seed','eps','min_samples'}
    if set(params)-allowed: fail('invalid_parameters','包含不支持的聚类参数')
    target=request.get('user_id')
    if target is not None and (isinstance(target,bool) or not isinstance(target,int)): fail('invalid_input','用户 ID 须为整数')
    if target is not None and str(target) not in entities['users']: fail('unknown_user',f'用户 {target} 不存在于清洗数据')
    if target is not None and request.get('user_ids') is not None and target not in request['user_ids']: fail('user_not_in_cohort','目标用户不在指定集合中')
    if request.get('model_id'):
        found=assets.find('',request['model_id'])
        if not found: fail('model_unavailable','指定聚类结果不存在')
        folder,meta=found
        if meta['dataset_id']!=dataset['id'] or meta['task']!=algorithm: fail('version_mismatch','聚类结果的算法或数据版本不一致')
        with (folder/'model.pkl').open('rb') as f: bundle=pickle.load(f)
        result=read_json(folder/'result.json')
        result['memberships']=membership(bundle['users'],bundle['labels'],bundle['reasons'],request,entities)
        return result,folder
    progress('构建训练期类型偏好画像')
    users,x,counts,reasons=build(data,entities,dataset['t1'],request)
    key=key_for(algorithm,dataset['id'],params,users.tolist())
    found=assets.find(key,request.get('model_id'))
    if found and (found[1]['dataset_id']!=dataset['id'] or found[1]['task']!=algorithm): fail('version_mismatch','聚类结果的算法或数据版本不一致')
    if request.get('model_id') and not found: fail('model_unavailable','指定聚类结果不存在')
    if not found:
        if request.get('mode') in ['predict','lookup']: fail('model_unavailable','该配置的聚类结果尚未生成')
        progress('运行 KMeans' if algorithm=='kmeans' else '运行 DBSCAN')
        model,labels,actual,temporal=select_cluster(x,users,data,entities,dataset,algorithm,params)
        actual['min_history']=params.get('min_history',5)
        met,note=metrics(x,labels,actual.get('seed',42))
        groups=summaries(users,x,labels,data,entities,dataset['t1'])
        folder,meta=assets.create(algorithm,key,dataset,actual,'KMeans 群体' if algorithm=='kmeans' else 'DBSCAN 群体')
        rows_file(folder,'groups',groups)
        rows_file(folder,'memberships',[{'user_id':int(u),'group':int(g),'noise':bool(g<0),'status':'completed'} for u,g in zip(users,labels)]+[{'user_id':int(u),'group':None,'noise':False,'status':reason} for u,reason in reasons.items()])
        rows_file(folder,'profiles',({'user_id':int(u),'history_count':int(n),'preferences':row.tolist()} for u,n,row in zip(users,counts,x)))
        result=base_result(dataset,'clustering','KMeans' if algorithm=='kmeans' else 'DBSCAN')
        result.update(groups=groups[:12],temporal_metrics=temporal,metrics=met,metric_note=note,files=['memberships.csv','profiles.csv','groups.csv'],
            limitations=result['limitations']+['群体只描述训练截止前的高分类型偏好，不能视为固定性格。',f'历史不足或无高分信号的用户 {len(reasons)} 人未分组，不删除用户。'])
        found=assets.publish(folder,meta,result,{'users':users,'x':x,'labels':labels,'reasons':reasons,'model':model})
    folder,_=found
    with (folder/'model.pkl').open('rb') as f: bundle=pickle.load(f)
    result=read_json(folder/'result.json')
    result['memberships']=membership(bundle['users'],bundle['labels'],bundle['reasons'],request,entities)
    return result,folder
