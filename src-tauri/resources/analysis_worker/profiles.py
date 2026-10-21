from collections import Counter
import numpy as np
from .dataset import GENRES
from .common import fail, number


def build(data,entities,t1,request):
    params=request.get('parameters') or {}
    minimum=number(params,'min_history',5,1,10000,True)
    known=sorted(map(int,entities['users']))
    requested=request.get('user_ids')
    if requested is not None and (not isinstance(requested,list) or not requested or len(requested)>10000 or any(isinstance(x,bool) or not isinstance(x,int) for x in requested)):
        fail('invalid_input','用户集合须为非空整数列表，最多 10000 人')
    cohort=sorted(set(requested)) if requested is not None else known
    unknown=[u for u in cohort if str(u) not in entities['users']]
    if unknown: fail('unknown_user',f'用户 {unknown[0]} 不存在于清洗数据')
    index={u:i for i,u in enumerate(cohort)}
    matrix=np.zeros((len(cohort),len(GENRES)),dtype=np.float64)
    counts=np.zeros(len(cohort),dtype=np.int64)
    for u,m,r,t in data:
        if t>t1: break
        i=index.get(int(u))
        if i is None: continue
        counts[i]+=1
        if r>=4:
            genres=entities['movies'][str(m)][2].split('|')
            for g in genres: matrix[i,GENRES.index(g)]+=1/len(genres)
    totals=matrix.sum(axis=1)
    reasons={u:('insufficient_history' if counts[i]<minimum else 'no_preference_signal') for i,u in enumerate(cohort) if counts[i]<minimum or totals[i]==0}
    mask=(counts>=minimum)&(totals>0)
    users=np.asarray(cohort,dtype=np.int64)[mask]
    if mask.sum()<3: fail('insufficient_history','有足够训练期历史和高分偏好的用户不足 3 人')
    return users,matrix[mask]/totals[mask,None],counts[mask],reasons


def summaries(users,x,labels,data,entities,t1):
    groups=[]
    for label in sorted(set(labels)):
        if label<0: continue
        mask=labels==label; members=set(map(int,users[mask])); center=x[mask].mean(axis=0)
        movie_counts=Counter(int(m) for u,m,r,t in data if t<=t1 and int(u) in members and r>=4)
        groups.append({'group':int(label),'size':int(mask.sum()),'genres':[{'name':GENRES[i],'share':float(center[i])} for i in np.argsort(center)[::-1][:3]],
            'movies':[{'movie_id':mid,'title':entities['movies'][str(mid)][1]} for mid,_ in movie_counts.most_common(3)]})
    return groups


def membership(users,labels,reasons,request,entities):
    lookup={int(u):int(g) for u,g in zip(users,labels)}
    target=request.get('user_id')
    if target is not None:
        if isinstance(target,bool) or not isinstance(target,int): fail('invalid_input','用户 ID 须为整数')
        if str(target) not in entities['users']: fail('unknown_user',f'用户 {target} 不存在于清洗数据')
        if target not in lookup and target not in reasons: fail('user_not_in_cohort','该用户不在此用户集合内')
        selected=[target]
    else: selected=list(lookup)[:12]+list(reasons)[:3]
    return [{'user_id':int(u),'group':lookup.get(u),'noise':lookup.get(u)==-1,'status':reasons.get(u,'completed')} for u in selected]
