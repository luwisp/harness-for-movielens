import pickle
import time
from html import escape
import numpy as np
from scipy.linalg import pinv
from sklearn.metrics import adjusted_rand_score
from .common import fail,number,key_for,base_result,read_json,rows_file,progress
from .profiles import build,summaries
from .clustering import cluster,metrics,select_cluster,period_loss
from .dataset import GENRES


def svd_reduce(x,q,decomposition=None):
    _,singular,vt=np.linalg.svd(x,full_matrices=False) if decomposition is None else decomposition
    basis=vt[:q]
    z=x@basis.T
    return z,z@basis,{'basis':basis,'singular_values':singular,'rank':int(np.linalg.matrix_rank(z))}


def cur_order(x,seed):
    rng=np.random.default_rng(seed)
    col=np.sum(x*x,axis=0); row=np.sum(x*x,axis=1)
    columns=np.flatnonzero(col>0); rows=np.flatnonzero(row>0)
    if not len(columns) or not len(rows): fail('no_preference_signal','画像矩阵没有可降维的信号')
    return (rng.choice(columns,len(columns),replace=False,p=col[columns]/col[columns].sum()),
            rng.choice(rows,len(rows),replace=False,p=row[rows]/row[rows].sum()))


def cur_reduce(x,q,seed=42,row_samples=None,order=None):
    cols,rows=cur_order(x,seed) if order is None else order
    if q>len(cols): fail('invalid_dimensions','非零类型列少于请求维数')
    count=min(len(rows),row_samples if row_samples is not None else 2*q)
    j=cols[:q]; i=rows[:count]
    c=x[:,j]; r=x[i,:]
    middle=pinv(c)@x@pinv(r)
    decoder=middle@r
    return c,c@decoder,{'columns':j,'rows':i,'middle':middle,'sample_rows':r,'decoder':decoder,'rank':int(np.linalg.matrix_rank(c))}


def reconstruction(x,reconstructed):
    norm=float(np.sum(x*x))
    if norm==0: fail('no_preference_signal','零矩阵无法计算信息保留情况')
    error=float(np.sum((x-reconstructed)**2)/norm)
    return {'relative_error':error,'retained':1-error}


def coordinates(users,z,labels):
    return [{'user_id':int(u),'x':float(row[0]),'y':float(row[1]) if z.shape[1]>1 else 0.,'group':int(g),'noise':bool(g<0)} for u,row,g in zip(users,z,labels)]


def svg(path,original,reduced):
    colors=['#467b76','#ce8c58','#6778af','#b16e92','#6d9f57','#b5a44e']
    parts=['<svg xmlns="http://www.w3.org/2000/svg" width="900" height="380" viewBox="0 0 900 380"><rect width="900" height="380" fill="white"/>']
    for panel,(name,rows) in enumerate([('原始画像：二维参考视图',original),('降维表示',reduced)]):
        left=45+panel*450
        parts.append(f'<text x="{left}" y="30" font-family="sans-serif" font-size="17">{escape(name)}</text>')
        xx=[r['x'] for r in rows]; yy=[r['y'] for r in rows]
        xmin,xmax=min(xx),max(xx); ymin,ymax=min(yy),max(yy)
        for row in rows:
            x=left+(row['x']-xmin)/max(xmax-xmin,1e-12)*365
            y=335-(row['y']-ymin)/max(ymax-ymin,1e-12)*270
            color='#9ca3aa' if row['noise'] else colors[row['group']%len(colors)]
            parts.append(f'<circle cx="{x:.3f}" cy="{y:.3f}" r="2.3" fill="{color}" opacity=".65"><title>用户 {row["user_id"]}</title></circle>')
    parts.append('</svg>')
    path.write_text(''.join(parts),encoding='utf-8')


def run(request,dataset,data,entities,assets,algorithm):
    mode=request.get('mode','fit')
    if mode not in ['fit','predict','lookup']: fail('invalid_parameters','运行模式无效')
    if request.get('model_id') and (request.get('parameters') or request.get('user_ids')):
        fail('invalid_parameters','指定已有模型时不能同时更改参数或用户集合，请单独训练新模型')
    params=request.get('parameters') or {}
    allowed={'dimensions','information_threshold','seed','min_history','row_samples'}
    if set(params)-allowed: fail('invalid_parameters','包含不支持的降维参数')
    if 'dimensions' in params and 'information_threshold' in params: fail('invalid_parameters','请指定维数或信息阈值中的一项')
    if request.get('model_id'):
        found=assets.find('',request['model_id'])
        if not found: fail('model_unavailable','指定降维结果不存在')
        if found[1]['task']!=algorithm or found[1]['dataset_id']!=dataset['id']: fail('version_mismatch','降维结果算法或数据版本不一致')
        return read_json(found[0]/'result.json'),found[0]
    users,x,counts,reasons=build(data,entities,dataset['t1'],request)
    seed=number(params,'seed',42,0,2147483647,True)
    maxq=min(x.shape)
    requested=number(params,'dimensions',min(2,maxq),1,maxq,True)
    threshold=number(params,'information_threshold',.9, .01,1.) if 'information_threshold' in params else None
    rows=number(params,'row_samples',min(len(x),2*requested),1,len(x),True) if 'row_samples' in params else None
    comparator=request.get('clustering_algorithm','kmeans')
    cp=request.get('clustering_parameters') or {}
    if comparator not in ['kmeans','dbscan']: fail('invalid_parameters','不支持的对照聚类算法')
    if set(cp)-({'n_clusters','n_init','seed'} if comparator=='kmeans' else {'eps','min_samples','seed'}): fail('invalid_parameters','包含不支持的对照聚类参数')
    key=key_for(algorithm,dataset['id'],{'reduction':params,'clustering':comparator,'clustering_parameters':cp},users.tolist())
    found=assets.find(key,request.get('model_id'))
    if found and (found[1]['task']!=algorithm or found[1]['dataset_id']!=dataset['id']): fail('version_mismatch','降维结果算法或数据版本不一致')
    if request.get('model_id') and not found: fail('model_unavailable','指定降维结果不存在')
    if found: return read_json(found[0]/'result.json'),found[0]
    if request.get('mode') in ['predict','lookup']: fail('model_unavailable','该配置的降维结果尚未生成')
    progress('计算 SVD 与原始空间基线' if algorithm=='svd' else '按训练期能量采样 CUR 行列')
    started=time.perf_counter()
    decomposition=np.linalg.svd(x,full_matrices=False)
    order=cur_order(x,seed) if algorithm=='cur' else None
    max_possible=min(maxq,len(order[0])) if order is not None else maxq
    for q in (range(1,max_possible+1) if threshold is not None else [requested]):
        z,rebuilt,model=svd_reduce(x,q,decomposition) if algorithm=='svd' else cur_reduce(x,q,seed,rows,order)
        info=reconstruction(x,rebuilt)
        if threshold is None or info['retained']+1e-12>=threshold: break
    if threshold is not None and info['retained']+1e-12<threshold:
        fail('threshold_unmet',f'允许维数内信息保留率最多达到 {info["retained"]:.2%}，未达到 {threshold:.2%}')
    reduce_seconds=time.perf_counter()-started
    progress('比较原始画像与降维表示上的同配置聚类')
    clustering_started=time.perf_counter()
    base_model,base_labels,cluster_params,temporal=select_cluster(x,users,data,entities,dataset,comparator,cp)
    original_cluster_seconds=time.perf_counter()-clustering_started
    reduced_started=time.perf_counter()
    reduced_model,labels,_=cluster(z,comparator,cluster_params)
    reduced_cluster_seconds=time.perf_counter()-reduced_started
    clustering_seconds=time.perf_counter()-clustering_started
    original_metrics,original_note=metrics(x,base_labels,seed)
    reduced_metrics,reduced_note=metrics(z,labels,seed)
    common=(base_labels>=0)&(labels>=0)
    common_metrics,common_note=metrics(x[common],labels[common],seed) if common.any() else ({'silhouette':None,'davies_bouldin':None},'无共同的非噪声用户')
    original_common,_=metrics(x[common],base_labels[common],seed) if common.any() else ({'silhouette':None,'davies_bouldin':None},None)
    agreement=float(adjusted_rand_score(base_labels[common],labels[common])) if common.sum()>1 else None
    preview=decomposition[0][:,:2]*decomposition[1][:2]
    original_plot=coordinates(users,preview,base_labels)
    reduced_plot=coordinates(users,z,labels)
    name='截断 SVD' if algorithm=='svd' else 'CUR'
    actual={'dimensions':q,'seed':seed,'min_history':params.get('min_history',5),'clustering_algorithm':comparator,'clustering_parameters':cluster_params}
    if threshold is not None: actual['information_threshold']=threshold
    if algorithm=='cur': actual['row_samples']=len(model['rows'])
    folder,meta=assets.create(algorithm,key,dataset,actual,name)
    rows_file(folder,'coordinates',({'user_id':int(u),'coordinates':row.tolist(),'group':int(g),'noise':bool(g<0)} for u,row,g in zip(users,z,labels)))
    rows_file(folder,'original_plot',original_plot); rows_file(folder,'reduced_plot',reduced_plot)
    rows_file(folder,'profiles',({'user_id':int(u),'preferences':row.tolist(),'history_count':int(c)} for u,row,c in zip(users,x,counts)))
    svg(folder/'projection.svg',original_plot,reduced_plot)
    comparison=[{'space':'原始画像',**original_metrics},{'space':'降维表示',**reduced_metrics}]
    rows_file(folder,'comparison',comparison)
    result=base_result(dataset,'reduction',name)
    result.update(original_dimensions=x.shape[1],dimensions=q,information=info,
        axes=(['潜在方向 1','潜在方向 2'] if algorithm=='svd' else [GENRES[i] for i in model['columns'][:2]]),
        temporal_metrics={'original':temporal,'reduced':{'validation':period_loss(users,x,labels,data,entities,dataset['t1'],dataset['t2']),'test':period_loss(users,x,labels,data,entities,dataset['t2'],np.iinfo(np.int64).max)}},
        comparison=comparison,comparison_notes=[original_note,reduced_note],
        reference_metrics={'original':original_common,'reduced':common_metrics,'note':common_note,'agreement':agreement},
        files=['coordinates.csv','comparison.csv','projection.svg'],
        resources={'reduction_seconds':reduce_seconds,'clustering_seconds':clustering_seconds,'original_cluster_seconds':original_cluster_seconds,'reduced_cluster_seconds':reduced_cluster_seconds,'original_matrix_bytes':int(x.nbytes),'reduced_matrix_bytes':int(z.nbytes)},
        limitations=result['limitations']+['二维图仅用于展示，不能单凭点的远近判断全部维度的聚类质量。','各空间的内部指标受距离变化影响，需结合原始空间对照及覆盖率。'])
    if model['rank']<q: result['limitations'].append(f'输出 {q} 列坐标，有效秩为 {model["rank"]}，部分方向相关。')
    if algorithm=='cur':
        result['selected_genres']=[GENRES[i] for i in model['columns']]
        result['limitations'].append(f'CUR 有 {int(np.sum(np.linalg.norm(z,axis=1)==0))} 名用户在选中类型上为零向量；保留这些用户。')
    if q==x.shape[1]: result['limitations'].append('实际维数等于原始维数，本次未压缩。')
    if q==1: result['limitations'].append('实际输出为一维，图中纵轴为显示用常数。')
    bundle={'model':model,'users':users,'coordinates':z,'labels':labels,'original_labels':base_labels,'cluster':reduced_model,'original_cluster':base_model}
    result['resources']['model_bytes']=len(pickle.dumps(bundle,protocol=5))
    found=assets.publish(folder,meta,result,bundle)
    return read_json(found[0]/'result.json'),found[0]
