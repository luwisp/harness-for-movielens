import pickle
from pathlib import Path
import numpy as np
from sklearn.compose import ColumnTransformer
from sklearn.pipeline import make_pipeline
from sklearn.preprocessing import StandardScaler
from sklearn.linear_model import LogisticRegression
from sklearn.tree import DecisionTreeClassifier
from sklearn.metrics import precision_recall_curve, auc, roc_auc_score
from .common import fail, progress, number, key_for, base_result, read_json, rows_file
from .features import FeatureBuilder


def scores(y,p,threshold):
    if len(y)==0:
        return {k:None for k in ['precision','recall','f1','roc_auc','pr_auc']}, '该时间段没有样本'
    pred=p>=threshold
    tp=int(np.sum(pred&(y==1))); fp=int(np.sum(pred&(y==0))); fn=int(np.sum(~pred&(y==1)))
    both=len(np.unique(y))==2
    precision,recall,_=precision_recall_curve(y,p) if np.any(y) else (None,None,None)
    return {'precision':tp/(tp+fp) if tp+fp else None,'recall':tp/(tp+fn) if tp+fn else None,
            'f1':2*tp/(2*tp+fp+fn) if 2*tp+fp+fn else None,
            'roc_auc':float(roc_auc_score(y,p)) if both else None,
            'pr_auc':float(auc(recall,precision)) if both else None}, (None if both else '只有单个类别，部分指标不适用')


def select_threshold(y,p):
    if len(np.unique(y))<2: return .5
    precision,recall,thresholds=precision_recall_curve(y,p)
    f=2*precision[:-1]*recall[:-1]/np.maximum(precision[:-1]+recall[:-1],1e-12)
    return float(thresholds[int(np.argmax(f))]) if len(thresholds) else .5


def feature_cache(root,data,entities,t1):
    root=Path(root)
    if (root/'features.complete').is_file() and (root/'features.npy').is_file():
        with (root/'features.pkl').open('rb') as f: builder=pickle.load(f)
        return np.load(root/'features.npy',mmap_mode='r'),builder
    progress('按时间生成训练特征与冻结历史')
    builder=FeatureBuilder(); matrix=builder.fit_transform(data,entities,t1)
    np.save(root/'features.npy',matrix)
    with (root/'features.pkl').open('wb') as f: pickle.dump(builder,f,protocol=5)
    (root/'features.complete').write_text('causal-v1')
    return matrix,builder


def run(request,dataset,data,entities,assets,algorithm):
    uid,mid=request.get('user_id'),request.get('movie_id')
    if (uid is None)!=(mid is None): fail('invalid_input','分类预测需要同时提供用户 ID 和电影 ID')
    if uid is not None:
        if isinstance(uid,bool) or not isinstance(uid,int) or isinstance(mid,bool) or not isinstance(mid,int): fail('invalid_input','用户和电影 ID 须为整数')
        if str(uid) not in entities['users']: fail('unknown_user',f'用户 {uid} 不存在于清洗数据')
        if str(mid) not in entities['movies']: fail('unknown_movie',f'电影 {mid} 不存在于清洗数据')
    mode=request.get('mode','fit')
    if mode not in ['fit','predict','lookup']: fail('invalid_parameters','运行模式无效')
    if request.get('model_id') and (request.get('parameters') or request.get('user_ids')):
        fail('invalid_parameters','指定已有模型时不能同时更改参数或用户集合，请单独训练新模型')
    params=request.get('parameters') or {}
    allowed={'seed','C','threshold'} if algorithm=='logistic' else {'seed','max_depth','min_samples_leaf','ccp_alpha','threshold'}
    if set(params)-allowed: fail('invalid_parameters','包含不支持的分类参数')
    seed=number(params,'seed',42,0,2147483647,True)
    for k,lo,hi in [('C',.0001,1000),('max_depth',1,64),('min_samples_leaf',1,10000),('ccp_alpha',0,1),('threshold',0,1)]:
        if k in params: number(params,k,0,lo,hi,k in ['max_depth','min_samples_leaf'])
    cache_params={'seed':seed,**params}
    key=key_for(algorithm,dataset['id'],cache_params)
    model_id=request.get('model_id')
    found=assets.find(key,model_id)
    if found and (found[1]['task']!=algorithm or found[1]['dataset_id']!=dataset['id']):
        fail('version_mismatch','指定模型的算法或数据集版本不一致')
    if model_id and not found: fail('model_unavailable','指定模型不存在')
    if not found:
        if request.get('mode') in ['predict','lookup']: fail('model_unavailable','模型尚未训练，请先运行训练评估')
        x,builder=feature_cache(request['dataset_dir'],data,entities,dataset['t1'])
        train=data[:,3]<=dataset['t1']; val=(data[:,3]>dataset['t1'])&(data[:,3]<=dataset['t2']); test=data[:,3]>dataset['t2']
        y=(data[:,2]>=4).astype(np.int8)
        if len(np.unique(y[train]))<2: fail('single_class','训练期只有一个类别，无法建立有效分类对照')
        if not val.any(): fail('validation_unavailable','验证期没有样本，无法选择参数和阈值')
        candidates=([{'C':c} for c in ([params['C']] if 'C' in params else [.1,1.,10.])]
            if algorithm=='logistic' else [{'max_depth':d,'min_samples_leaf':n,'ccp_alpha':params.get('ccp_alpha',0.)}
                for d in ([params['max_depth']] if 'max_depth' in params else [4,8,12])
                for n in ([params['min_samples_leaf']] if 'min_samples_leaf' in params else [10,50])])
        best=None
        for index,candidate in enumerate(candidates):
            progress(f'训练与验证{ "逻辑回归" if algorithm=="logistic" else "CART 决策树"}：方案 {index+1}/{len(candidates)}')
            if algorithm=='logistic':
                model=make_pipeline(ColumnTransformer([('numeric',StandardScaler(),list(range(x.shape[1]-4,x.shape[1])))],remainder='passthrough'),
                    LogisticRegression(C=candidate['C'],max_iter=500,random_state=seed))
            else: model=DecisionTreeClassifier(**candidate,criterion='gini',random_state=seed)
            model.fit(x[train],y[train]); p=model.predict_proba(x[val])[:,1]
            threshold=params.get('threshold',select_threshold(y[val],p))
            metrics,reason=scores(y[val],p,threshold)
            rank=(metrics['pr_auc'] if metrics['pr_auc'] is not None else -1,metrics['f1'] if metrics['f1'] is not None else -1)
            if best is None or rank>best[0]: best=(rank,model,threshold,candidate,metrics,reason)
        _,model,threshold,selected,val_metrics,val_reason=best
        progress('冻结模型并执行最终测试')
        pt=model.predict_proba(x[test])[:,1] if test.any() else np.array([])
        test_metrics,test_reason=scores(y[test],pt,threshold)
        actual={'seed':seed,**selected,'threshold':threshold}
        if algorithm=='tree': actual['criterion']='gini'
        folder,meta=assets.create(algorithm,key,dataset,actual,'逻辑回归' if algorithm=='logistic' else 'CART 决策树')
        result=base_result(dataset,'classification','逻辑回归' if algorithm=='logistic' else 'CART 决策树')
        result.update(metrics={'validation':val_metrics,'test':test_metrics},metric_notes={'validation':val_reason,'test':test_reason},files=[],
                      limitations=result['limitations']+['仅使用训练截止时间之前的历史；概率为未校准的模型估计，不能解释为已验证的真实置信度。'])
        if not test.any(): result['limitations'].append('测试期没有样本，最终测试指标未完成。')
        rows_file(folder,'predictions',({'用户':int(row[0]),'电影':int(row[1]),'实际高分':int(row[2]>=4),'预测高分概率':float(p),'预测高分':bool(p>=threshold)} for row,p in zip(data[test],pt)))
        found=assets.publish(folder,meta,result,{'model':model,'features':builder,'threshold':threshold})
    folder,meta=found
    result=read_json(folder/'result.json')
    uid,mid=request.get('user_id'),request.get('movie_id')
    if (uid is None)!=(mid is None): fail('invalid_input','分类预测需要同时提供用户 ID 和电影 ID')
    if uid is not None:
        if isinstance(uid,bool) or not isinstance(uid,int) or isinstance(mid,bool) or not isinstance(mid,int): fail('invalid_input','用户和电影 ID 须为整数')
        with (folder/'model.pkl').open('rb') as f: bundle=pickle.load(f)
        builder=bundle['features']; pair=builder.transform([(uid,mid)])
        probability=float(bundle['model'].predict_proba(pair)[0,1]); threshold=bundle['threshold']
        cold=bool(builder.cold_start(uid,mid))
        result['target']={'user_id':uid,'movie_id':mid,'movie_title':entities['movies'][str(mid)][1],
            'probability':probability,'positive':probability>=threshold,'cold_start':cold}
        if cold: result['limitations'].append('该用户或电影在训练期没有历史，预测使用训练期先验，属于冷启动。')
    return result,folder
