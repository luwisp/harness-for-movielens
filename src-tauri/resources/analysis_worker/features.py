import numpy as np
from .common import fail, progress

class FeatureBuilder:
    """Training prefixes are strictly causal; validation/test use a frozen T1 snapshot."""
    def fit_transform(self, ratings, entities, t1):
        self.entities = entities
        self.user_ids = sorted(map(int, entities['users']))
        self.movie_ids = sorted(map(int, entities['movies']))
        self.ui = {u:i for i,u in enumerate(self.user_ids)}
        self.mi = {m:i for i,m in enumerate(self.movie_ids)}
        train = ratings[ratings[:,3] <= t1]
        self.ages = sorted({entities['users'][str(u)][2] for u in train[:,0]})
        self.genres = sorted({g for m in train[:,1] for g in entities['movies'][str(m)][2].split('|')})
        self.user_sums = np.zeros(len(self.ui)); self.user_counts = np.zeros(len(self.ui))
        self.movie_sums = np.zeros(len(self.mi)); self.movie_counts = np.zeros(len(self.mi))
        self.total_sum = self.total_count = 0.
        out = np.zeros((len(ratings),len(self.ages)+len(self.genres)+4), dtype=np.float32)
        start = 0
        while start < len(train):
            end = int(np.searchsorted(train[:,3], train[start,3], side='right'))
            batch = train[start:end]
            out[start:end] = self.transform(batch[:,:2])
            ui = np.array([self.ui[int(x)] for x in batch[:,0]])
            mi = np.array([self.mi[int(x)] for x in batch[:,1]])
            np.add.at(self.user_sums, ui, batch[:,2]); np.add.at(self.user_counts, ui, 1)
            np.add.at(self.movie_sums, mi, batch[:,2]); np.add.at(self.movie_counts, mi, 1)
            self.total_sum += batch[:,2].sum(); self.total_count += len(batch)
            start = end
        if len(train)<len(ratings):
            out[len(train):] = self.transform(ratings[len(train):,:2])
        return out

    def transform(self, pairs):
        out = np.zeros((len(pairs),len(self.ages)+len(self.genres)+4), dtype=np.float32)
        prior = self.total_sum/self.total_count if self.total_count else 3.
        for i,(user,movie) in enumerate(pairs):
            u,m = int(user),int(movie)
            if u not in self.ui: fail('unknown_user', f'用户 {u} 不存在于清洗数据')
            if m not in self.mi: fail('unknown_movie', f'电影 {m} 不存在于清洗数据')
            age = self.entities['users'][str(u)][2]
            if age in self.ages: out[i,self.ages.index(age)] = 1
            for g in self.entities['movies'][str(m)][2].split('|'):
                if g in self.genres: out[i,len(self.ages)+self.genres.index(g)] = 1
            ui,mi = self.ui[u],self.mi[m]
            out[i,-4:] = [(self.user_sums[ui]+5*prior)/(self.user_counts[ui]+5), np.log1p(self.user_counts[ui]),
                          (self.movie_sums[mi]+5*prior)/(self.movie_counts[mi]+5), np.log1p(self.movie_counts[mi])]
        return out

    def cold_start(self,u,m):
        return self.user_counts[self.ui[u]] == 0 or self.movie_counts[self.mi[m]] == 0
