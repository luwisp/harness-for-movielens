import multiprocessing
from analysis_worker.__main__ import entrypoint

if __name__ == '__main__':
    multiprocessing.freeze_support()
    entrypoint()
